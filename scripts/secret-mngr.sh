#!/usr/bin/env bash
# secret-mngr.sh — manage secrets across GCP Secret Manager and GitHub Actions.
# Subcommands: set, get, status, rm. See `secret-mngr --help` for usage.

set -euo pipefail

# ── logging ──────────────────────────────────────────────────────────────────
if [[ -t 1 ]] && [[ -z "${NO_COLOR:-}" ]]; then
    C_RED=$'\033[31m'; C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'
    C_BOLD=$'\033[1m'; C_DIM=$'\033[2m'; C_RESET=$'\033[0m'
else
    C_RED=''; C_GREEN=''; C_YELLOW=''; C_BOLD=''; C_DIM=''; C_RESET=''
fi

ok()   { printf "  ${C_GREEN}✓${C_RESET} %s\n" "$1"; }
miss() { printf "  ${C_RED}✗${C_RESET} %s ${C_DIM}— %s${C_RESET}\n" "$1" "${2:-}"; }
warn() { printf "  ${C_YELLOW}!${C_RESET} %s ${C_DIM}— %s${C_RESET}\n" "$1" "${2:-}"; }
info() { printf "${C_BOLD}==>${C_RESET} %s\n" "$1"; }
err()  { printf "${C_RED}${C_BOLD}error:${C_RESET} %s\n" "$1" >&2; }
die()  { err "$1"; exit "${2:-1}"; }

usage() {
    cat <<'EOF'
secret-mngr — manage secrets across GCP Secret Manager and GitHub Actions.

usage:
  secret-mngr set <name>     [--project P] [--repo OWNER/NAME] [--only gcp|gh] [--gh-name N]
                             value is read from stdin (e.g. `pbpaste | secret-mngr set foo`)
  secret-mngr get <name>     [--project P]
                             prints GCP value to stdout (GH cannot be read back via API)
  secret-mngr status [name]  [--project P] [--repo OWNER/NAME]
                             with name: per-backend status; without: list all + drift
  secret-mngr rm <name>      [--project P] [--repo OWNER/NAME] [--only gcp|gh] [--yes]

defaults:
  --project   $SECRET_MNGR_GCP_PROJECT, else `gcloud config get-value project`
  --repo      parsed from `git remote get-url origin`
  --gh-name   auto-uppercase of <name>; pass --gh-name to override
EOF
}

# ── name normalization ───────────────────────────────────────────────────────
gcp_name() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

gh_name() {
    local override="${1:-}" name="$2"
    if [[ -n "$override" ]]; then
        printf '%s' "$override"
    else
        printf '%s' "$name" | tr '[:lower:]-' '[:upper:]_'
    fi
}

validate_gh_name() {
    [[ "$1" =~ ^[A-Z_][A-Z0-9_]*$ ]] || \
        die "GH secret name '$1' must match ^[A-Z_][A-Z0-9_]*\$ — pass --gh-name to override"
}

# ── default resolution ───────────────────────────────────────────────────────
resolve_project() {
    local p="${1:-}"
    if [[ -n "$p" ]]; then printf '%s' "$p"; return; fi
    if [[ -n "${SECRET_MNGR_GCP_PROJECT:-}" ]]; then
        printf '%s' "$SECRET_MNGR_GCP_PROJECT"; return
    fi
    p=$(gcloud config get-value project 2>/dev/null || true)
    [[ -n "$p" && "$p" != "(unset)" ]] || \
        die "no GCP project — pass --project, set SECRET_MNGR_GCP_PROJECT, or 'gcloud config set project ...'"
    printf '%s' "$p"
}

resolve_repo() {
    local r="${1:-}"
    if [[ -n "$r" ]]; then printf '%s' "$r"; return; fi
    local origin
    origin=$(git remote get-url origin 2>/dev/null) || \
        die "no GH repo — pass --repo OWNER/NAME or run inside a git checkout with origin set"
    origin="${origin#git@github.com:}"
    origin="${origin#https://github.com/}"
    origin="${origin#ssh://git@github.com/}"
    origin="${origin%.git}"
    [[ "$origin" == */* ]] || die "could not parse GH repo from origin: $origin"
    printf '%s' "$origin"
}

# ── tool / auth checks ───────────────────────────────────────────────────────
ensure_gcloud() {
    command -v gcloud >/dev/null 2>&1 || \
        die "gcloud not found — install: https://cloud.google.com/sdk/docs/install"
    gcloud auth list --filter='status:ACTIVE' --format='value(account)' 2>/dev/null | grep -q . || \
        die "gcloud is not authenticated — run: gcloud auth login"
}

ensure_gh() {
    command -v gh >/dev/null 2>&1 || \
        die "gh not found — install: brew install gh  (or https://cli.github.com)"
    gh auth status >/dev/null 2>&1 || \
        die "gh is not authenticated — run: gh auth login"
}

# ── GCP backend ──────────────────────────────────────────────────────────────
gcp_exists() {
    gcloud secrets describe "$1" --project="$2" >/dev/null 2>&1
}

gcp_set() {
    local name="$1" project="$2" file="$3"
    if gcp_exists "$name" "$project"; then
        gcloud secrets versions add "$name" --project="$project" --data-file="$file" >/dev/null
        ok "GCP $name (new version, project=$project)"
    else
        gcloud secrets create "$name" --project="$project" --data-file="$file" \
            --replication-policy=automatic >/dev/null
        ok "GCP $name (created, project=$project)"
    fi
}

gcp_get() {
    local name="$1" project="$2"
    gcp_exists "$name" "$project" || die "GCP secret '$name' not found in project '$project'"
    gcloud secrets versions access latest --secret="$name" --project="$project"
}

gcp_rm() {
    local name="$1" project="$2"
    if gcp_exists "$name" "$project"; then
        gcloud secrets delete "$name" --project="$project" --quiet >/dev/null
        ok "GCP $name (deleted, project=$project)"
    else
        warn "GCP $name" "not present in $project (skipping)"
    fi
}

gcp_describe() {
    local name="$1" project="$2"
    if ! gcp_exists "$name" "$project"; then
        printf '%s' "${C_RED}✗${C_RESET}"
        return
    fi
    local ver created
    ver=$(gcloud secrets versions list "$name" --project="$project" \
        --filter='state:ENABLED' --sort-by=~createTime --limit=1 \
        --format='value(name)' 2>/dev/null || true)
    created=$(gcloud secrets versions list "$name" --project="$project" \
        --filter='state:ENABLED' --sort-by=~createTime --limit=1 \
        --format='value(createTime)' 2>/dev/null || true)
    printf '%s v%s, created %s' "${C_GREEN}✓${C_RESET}" "${ver:-?}" "${created:-?}"
}

gcp_list() {
    gcloud secrets list --project="$1" --format='value(name)' 2>/dev/null
}

# ── GH backend ───────────────────────────────────────────────────────────────
gh_exists() {
    gh secret list --repo "$2" --json name -q '.[].name' 2>/dev/null | grep -qx "$1"
}

gh_set() {
    local name="$1" repo="$2" file="$3"
    gh secret set "$name" --repo "$repo" --body-file "$file" >/dev/null
    ok "GH  $name (repo=$repo)"
}

gh_rm() {
    local name="$1" repo="$2"
    if gh_exists "$name" "$repo"; then
        gh secret delete "$name" --repo "$repo" >/dev/null
        ok "GH  $name (deleted, repo=$repo)"
    else
        warn "GH $name" "not present in $repo (skipping)"
    fi
}

gh_describe() {
    local name="$1" repo="$2" updated
    updated=$(gh secret list --repo "$repo" --json name,updatedAt \
        -q ".[] | select(.name == \"$name\") | .updatedAt" 2>/dev/null || true)
    if [[ -z "$updated" ]]; then
        printf '%s' "${C_RED}✗${C_RESET}"
    else
        printf '%s updated %s' "${C_GREEN}✓${C_RESET}" "$updated"
    fi
}

gh_list() {
    gh secret list --repo "$1" --json name -q '.[].name' 2>/dev/null
}

# ── stdin capture (binary-safe via tmp file; key bytes never enter argv/env) ─
TMP=""
cleanup() {
    if [[ -n "$TMP" && -e "$TMP" ]]; then
        if command -v shred >/dev/null 2>&1; then
            shred -u "$TMP" 2>/dev/null || rm -f "$TMP"
        else
            rm -f "$TMP"
        fi
    fi
}

capture_stdin() {
    [[ -t 0 ]] && die "stdin is a TTY — pipe a value in, e.g. 'pbpaste | secret-mngr set NAME'"
    TMP=$(mktemp)
    trap cleanup EXIT
    cat > "$TMP"
    [[ -s "$TMP" ]] || die "stdin was empty — refusing to set an empty secret"
}

# ── flag parsing ─────────────────────────────────────────────────────────────
PROJECT=""
REPO=""
ONLY=""
GH_NAME_OVERRIDE=""
ASSUME_YES=0
REMAINING=()

parse_common() {
    REMAINING=()
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --project)   [[ $# -ge 2 ]] || die "--project requires a value" 2; PROJECT="$2"; shift 2 ;;
            --project=*) PROJECT="${1#*=}"; shift ;;
            --repo)      [[ $# -ge 2 ]] || die "--repo requires a value" 2; REPO="$2"; shift 2 ;;
            --repo=*)    REPO="${1#*=}"; shift ;;
            --only)      [[ $# -ge 2 ]] || die "--only requires a value" 2; ONLY="$2"; shift 2 ;;
            --only=*)    ONLY="${1#*=}"; shift ;;
            --gh-name)   [[ $# -ge 2 ]] || die "--gh-name requires a value" 2; GH_NAME_OVERRIDE="$2"; shift 2 ;;
            --gh-name=*) GH_NAME_OVERRIDE="${1#*=}"; shift ;;
            --yes|-y)    ASSUME_YES=1; shift ;;
            -h|--help)   usage; exit 0 ;;
            --)          shift; REMAINING+=("$@"); break ;;
            -*)          die "unknown flag: $1" 2 ;;
            *)           REMAINING+=("$1"); shift ;;
        esac
    done
    case "$ONLY" in ""|gcp|gh) ;; *) die "--only must be 'gcp' or 'gh' (got: $ONLY)" 2 ;; esac
}

want_gcp() { [[ -z "$ONLY" || "$ONLY" == "gcp" ]]; }
want_gh()  { [[ -z "$ONLY" || "$ONLY" == "gh"  ]]; }

# ── subcommands ──────────────────────────────────────────────────────────────
cmd_set() {
    parse_common "$@"
    [[ ${#REMAINING[@]} -ge 1 ]] || die "set requires <name>" 2
    local name="${REMAINING[0]}"
    local gcp_n gh_n
    gcp_n=$(gcp_name "$name")
    gh_n=$(gh_name "$GH_NAME_OVERRIDE" "$name")
    validate_gh_name "$gh_n"

    capture_stdin

    if want_gcp; then
        ensure_gcloud
        local project; project=$(resolve_project "$PROJECT")
        gcp_set "$gcp_n" "$project" "$TMP"
    fi
    if want_gh; then
        ensure_gh
        local repo; repo=$(resolve_repo "$REPO")
        gh_set "$gh_n" "$repo" "$TMP"
    fi
}

cmd_get() {
    parse_common "$@"
    [[ ${#REMAINING[@]} -ge 1 ]] || die "get requires <name>" 2
    ensure_gcloud
    local project; project=$(resolve_project "$PROJECT")
    gcp_get "$(gcp_name "${REMAINING[0]}")" "$project"
}

cmd_status() {
    parse_common "$@"
    if [[ ${#REMAINING[@]} -ge 1 ]]; then
        status_one "${REMAINING[0]}"
    else
        status_all
    fi
}

status_one() {
    local name="$1"
    local gcp_n gh_n
    gcp_n=$(gcp_name "$name")
    gh_n=$(gh_name "$GH_NAME_OVERRIDE" "$name")

    ensure_gcloud
    ensure_gh
    local project repo
    project=$(resolve_project "$PROJECT")
    repo=$(resolve_repo "$REPO")

    printf '%s\n' "$name"
    printf '  GCP (%s)  ' "$project"; gcp_describe "$gcp_n" "$project"; printf '\n'
    printf '  GH  (%s) '  "$repo";    gh_describe  "$gh_n"  "$repo";    printf '\n'
}

status_all() {
    ensure_gcloud
    ensure_gh
    local project repo
    project=$(resolve_project "$PROJECT")
    repo=$(resolve_repo "$REPO")

    info "GCP project: $project"
    info "GH  repo:    $repo"
    echo

    # Union by lowercased name; GH names get downcased so `FOO_BAR` in GH
    # lines up with `foo_bar` in GCP. Plain newline-delimited strings + grep
    # rather than associative arrays — keeps this bash 3.2-compatible (macOS).
    local gcp_names gh_names_raw gh_names_lc all
    gcp_names=$(gcp_list "$project" | grep -v '^$' || true)
    gh_names_raw=$(gh_list "$repo" | grep -v '^$' || true)
    gh_names_lc=$(printf '%s\n' "$gh_names_raw" | tr '[:upper:]' '[:lower:]')

    all=$(printf '%s\n%s\n' "$gcp_names" "$gh_names_lc" | grep -v '^$' | sort -u || true)
    if [[ -z "$all" ]]; then
        warn "no secrets found" "in either backend"
        return
    fi

    printf '%-40s %-4s %-4s\n' "NAME" "GCP" "GH"
    local n g h marker
    while IFS= read -r n; do
        [[ -n "$n" ]] || continue
        if printf '%s\n' "$gcp_names"   | grep -qx "$n"; then g="Y"; else g="N"; fi
        if printf '%s\n' "$gh_names_lc" | grep -qx "$n"; then h="Y"; else h="N"; fi
        if [[ "$g" == "Y" && "$h" == "Y" ]]; then
            marker=""
        elif [[ "$g" == "Y" ]]; then
            marker="  ${C_YELLOW}← drift (GCP only)${C_RESET}"
        else
            marker="  ${C_YELLOW}← drift (GH only)${C_RESET}"
        fi
        printf '%-40s %-4s %-4s%s\n' "$n" "$g" "$h" "$marker"
    done <<< "$all"
}

cmd_rm() {
    parse_common "$@"
    [[ ${#REMAINING[@]} -ge 1 ]] || die "rm requires <name>" 2
    local name="${REMAINING[0]}"
    local gcp_n gh_n
    gcp_n=$(gcp_name "$name")
    gh_n=$(gh_name "$GH_NAME_OVERRIDE" "$name")

    local project="" repo=""
    if want_gcp; then ensure_gcloud; project=$(resolve_project "$PROJECT"); fi
    if want_gh;  then ensure_gh;     repo=$(resolve_repo "$REPO");          fi

    info "will delete:"
    if want_gcp; then printf '  GCP %s  (project=%s)\n' "$gcp_n" "$project"; fi
    if want_gh;  then printf '  GH  %s  (repo=%s)\n'    "$gh_n"  "$repo";    fi

    if [[ "$ASSUME_YES" -ne 1 ]]; then
        printf 'delete? [y/N] '
        local reply
        read -r reply
        [[ "$reply" =~ ^[Yy]$ ]] || { echo "aborted."; exit 1; }
    fi

    if want_gcp; then gcp_rm "$gcp_n" "$project"; fi
    if want_gh;  then gh_rm  "$gh_n"  "$repo";    fi
}

# ── entrypoint ───────────────────────────────────────────────────────────────
main() {
    [[ $# -ge 1 ]] || { usage; exit 2; }
    local sub="$1"; shift
    case "$sub" in
        set)            cmd_set "$@" ;;
        get)            cmd_get "$@" ;;
        status)         cmd_status "$@" ;;
        rm)             cmd_rm "$@" ;;
        -h|--help|help) usage ;;
        *)              err "unknown subcommand: $sub"; usage; exit 2 ;;
    esac
}

main "$@"

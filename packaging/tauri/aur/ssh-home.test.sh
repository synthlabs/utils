#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
fixtures="$(mktemp -d /tmp/tauri-ssh-test.XXXXXXXX)"
server_pid=
cleanup() {
  if [[ -n "${server_pid}" ]]; then
    kill "${server_pid}" 2>/dev/null || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  rm -rf -- "${fixtures}"
}
trap cleanup EXIT
chmod 755 "${fixtures}"

fail() {
  cat "${fixtures}/server.log" >&2 || true
  echo "SSH fixture failure: $*" >&2
  exit 1
}

useradd --user-group --create-home fixture-aur
usermod --password '*' fixture-aur
git init --bare "${fixtures}/remote.git"
chown -R fixture-aur:fixture-aur "${fixtures}/remote.git"
mkdir "${fixtures}/client"
chmod 755 "${fixtures}/client"
ssh-keygen -q -t ed25519 -N '' -f "${fixtures}/host"
ssh-keygen -q -t ed25519 -N '' -f "${fixtures}/wrong-host"
ssh-keygen -q -t ed25519 -N '' -f "${fixtures}/client/id_ed25519"
cp "${fixtures}/client/id_ed25519.pub" "${fixtures}/authorized_keys"
chmod 644 "${fixtures}/authorized_keys"
printf '[127.0.0.1]:2222 %s\n' "$(cut -d' ' -f1,2 "${fixtures}/host.pub")" \
  >"${fixtures}/client/known_hosts"
cat >"${fixtures}/client/config" <<'EOF'
Host 127.0.0.1
  IdentityFile ~/.ssh/id_ed25519
  IdentitiesOnly yes
  StrictHostKeyChecking yes
  UserKnownHostsFile ~/.ssh/known_hosts
  GlobalKnownHostsFile /dev/null
  BatchMode yes
EOF
cat >"${fixtures}/sshd_config" <<EOF
Port 2222
ListenAddress 127.0.0.1
HostKey ${fixtures}/host
PidFile ${fixtures}/sshd.pid
AuthorizedKeysFile ${fixtures}/authorized_keys
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
StrictModes no
UsePAM no
AllowUsers fixture-aur
EOF
install -d -m755 /run/sshd /var/empty
/usr/bin/sshd -D -f "${fixtures}/sshd_config" -E "${fixtures}/server.log" &
server_pid=$!
ready=0
for _ in {1..50}; do
  if ssh-keyscan -p 2222 127.0.0.1 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.1
done
(( ready )) || fail 'fixture SSH server did not start'

mkdir "${fixtures}/project"
touch "${fixtures}/project/sample.pkg.tar.zst"
repository="ssh://fixture-aur@127.0.0.1:2222${fixtures}/remote.git"
check_access() {
  runuser -u "${build_user}" -- env HOME="${build_home}" \
    bash "${script_dir}/publish.sh" --check-access \
      --project-root "${fixtures}/project" \
      --package "${fixtures}/project/sample.pkg.tar.zst" \
      --release-tag app-1.0.0-test --pkgver 1.0.0 --pkgrel 1 \
      --aur-repository "${repository}" \
      --git-user-name Fixture --git-user-email fixture@example.invalid
}

for scenario in new existing; do
  build_uid=20001
  build_gid=20001
  if [[ "${scenario}" == existing ]]; then
    build_uid=20002
    build_gid=20002
    groupadd --gid "${build_gid}" existing-builder
    useradd --uid "${build_uid}" --gid "${build_gid}" \
      --home-dir "${fixtures}/original-home" --create-home existing-builder
  fi
  build_home="${fixtures}/${scenario}-home"
  build_user="$(bash "${script_dir}/prepare-home.sh" \
    "${build_uid}" "${build_gid}" "${build_home}" "${fixtures}/client")"
  [[ "$(getent passwd "${build_uid}" | cut -d: -f6)" == "${build_home}" ]] \
    || fail "${scenario}: passwd home differs from HOME"
  [[ "$(stat -c '%a' "${build_home}/.ssh")" == 700 ]] || fail 'SSH directory permissions'
  [[ "$(stat -c '%a' "${build_home}/.ssh/id_ed25519")" == 600 ]] || fail 'key permissions'
  check_access || fail "${scenario}: trusted SSH access was rejected"
  printf '[127.0.0.1]:2222 %s\n' "$(cut -d' ' -f1,2 "${fixtures}/wrong-host.pub")" \
    >"${build_home}/.ssh/known_hosts"
  if check_access >"${fixtures}/rejected.log" 2>&1; then
    fail "${scenario}: untrusted host key was accepted"
  fi
  grep -q 'Host key verification failed' "${fixtures}/rejected.log" \
    || fail "${scenario}: failure did not reject the host key"
  echo "passed: ${scenario} account, trusted access and rejected host key"
done

remote_refs="$(runuser -u fixture-aur -- git --git-dir="${fixtures}/remote.git" for-each-ref)"
[[ -z "${remote_refs}" ]] \
  || fail 'access checks changed the remote repository'
echo 'AUR SSH home and read-only access tests passed'

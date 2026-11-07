<script lang="ts">
	import { untrack, type Snippet } from 'svelte';
	import type {
		ModelManagerController,
		ModelManagerRow,
		ModelManagerSnapshot
	} from '../controller';
	import type { InstallationDto } from '../types';
	import {
		defaultModelManagerCopy,
		modelInstallationProgress,
		type ModelManagerControl,
		type ModelManagerCopy
	} from './copy';

	let {
		controller,
		copy = {},
		class: className = '',
		header,
		modelRow,
		action,
		progress
	}: {
		controller: ModelManagerController;
		copy?: Partial<ModelManagerCopy>;
		class?: string;
		header?: Snippet<[ModelManagerSnapshot]>;
		modelRow?: Snippet<[ModelManagerRow, ModelManagerControl[]]>;
		action?: Snippet<[ModelManagerControl]>;
		progress?: Snippet<[InstallationDto, ModelManagerControl | null]>;
	} = $props();
	let snapshot = $state<ModelManagerSnapshot>(untrack(() => controller.snapshot()));
	const text = $derived({ ...defaultModelManagerCopy, ...copy });
	const active = $derived(snapshot.installation !== null && !snapshot.installation.finished);
	const blocked = $derived(
		!snapshot.connected ||
			snapshot.disposing ||
			snapshot.pending !== null ||
			active ||
			!['ready', 'paused'].includes(snapshot.service?.phase ?? '')
	);
	const sections = $derived([
		{ id: 'installed', label: text.installed, rows: snapshot.installed, empty: text.noInstalled },
		{
			id: 'available',
			label: text.available,
			rows: snapshot.available,
			empty: snapshot.service?.catalog.catalog ? text.noAvailable : text.noCatalog
		},
		{ id: 'attention', label: text.attention, rows: snapshot.attention, empty: '' }
	]);

	$effect(() =>
		controller.subscribe((value) => {
			snapshot = value;
		})
	);

	function controls(row: ModelManagerRow): ModelManagerControl[] {
		const pair = row.pair;
		if (!pair || row.local?.state === 'unmanaged' || row.local?.state === 'busy') return [];
		const result: ModelManagerControl[] = [];
		if (!row.local)
			result.push({
				kind: 'install',
				label: text.install,
				disabled: blocked,
				onclick: () => {
					void controller.install(pair);
				}
			});
		else {
			result.push({
				kind: 'verify',
				label: text.verify,
				disabled: blocked,
				onclick: () => {
					void controller.verify(pair);
				}
			});
			if (row.catalogModel) {
				const replacement = row.model !== null && row.model.id !== row.catalogModel.id;
				result.push({
					kind: replacement ? 'replace' : 'repair',
					label: replacement ? text.replace : text.repair,
					disabled: blocked,
					onclick: () => {
						void controller.install(pair, replacement ? 'replace' : 'repair');
					}
				});
			}
			result.push({
				kind: 'remove',
				label: text.remove,
				disabled: blocked,
				onclick: () => {
					void controller.remove(pair);
				}
			});
		}
		result.push({
			kind: 'clear_downloads',
			label: text.clearDownloads,
			disabled: blocked || row.inUse,
			title: row.inUse ? text.cleanupBusy : undefined,
			onclick: () => {
				void controller.clearDownloads(pair);
			}
		});
		return result;
	}

	function cancelControl(): ModelManagerControl | null {
		return active
			? {
					kind: 'cancel',
					label: snapshot.cancelling ? text.cancelling : text.cancel,
					disabled: snapshot.cancelling || snapshot.disposing,
					onclick: () => {
						void controller.cancelInstallation();
					}
				}
			: null;
	}

	function installationLabel(operation: InstallationDto): string {
		if (!operation.finished)
			return ['completed', 'cancelled', 'failed'].includes(operation.progress.phase)
				? text.awaitingResult
				: text.installPhase(operation.progress.phase);
		if (operation.result) return text.installationComplete;
		return operation.error?.code === 'cancelled'
			? text.installationCancelled
			: text.installationFailed;
	}
</script>

{#snippet button(control: ModelManagerControl)}
	{#if action}
		{@render action(control)}
	{:else}
		<button
			type="button"
			class="border-input hover:bg-muted focus-visible:ring-ring rounded-md border px-3 py-1.5 text-sm focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
			data-model-action={control.kind}
			disabled={control.disabled}
			title={control.title}
			onclick={control.onclick}>{control.label}</button
		>
	{/if}
{/snippet}

<section
	class={`grid min-w-0 gap-4 ${className}`}
	data-model-manager
	aria-busy={snapshot.loading || snapshot.pending !== null}
>
	{#if header}
		{@render header(snapshot)}
	{:else}
		<header>
			<h2 class="text-lg font-semibold">{text.title}</h2>
			<p class="text-muted-foreground text-sm">{text.description}</p>
		</header>
	{/if}

	<div class="flex flex-wrap gap-2">
		{@render button({
			kind: 'refresh_catalog',
			label: text.refreshCatalog,
			disabled:
				!snapshot.connected ||
				snapshot.disposing ||
				snapshot.pending !== null ||
				!['ready', 'paused'].includes(snapshot.service?.phase ?? '') ||
				(snapshot.service?.catalog.refreshing ?? false),
			onclick: () => {
				void controller.refreshCatalog();
			}
		})}
		{@render button({
			kind: 'inventory',
			label: text.refreshModels,
			disabled:
				!snapshot.connected ||
				snapshot.disposing ||
				snapshot.pending !== null ||
				!['ready', 'paused'].includes(snapshot.service?.phase ?? ''),
			onclick: () => {
				void controller.refreshModels();
			}
		})}
	</div>

	{#if !snapshot.connected}<p role="status">{text.notConnected}</p>{/if}
	{#if snapshot.loading}<p class="text-muted-foreground text-sm" role="status">
			{text.loading}
		</p>{/if}
	{#if snapshot.disposing}<p role="status">{text.disposing}</p>
	{:else if snapshot.pending?.stage === 'confirming'}<p role="status">{text.confirming}</p>{/if}
	{#if snapshot.service && snapshot.service.phase !== 'ready'}
		<div class="grid gap-2" data-model-service-phase={snapshot.service.phase}>
			<p role="status">{text.servicePhase(snapshot.service.phase)}</p>
			{#if snapshot.service.phase === 'unavailable'}
				{@render button({
					kind: 'retry_initialize',
					label: text.retryInitialize,
					disabled: snapshot.disposing || snapshot.pending !== null,
					onclick: () => {
						void controller.retryInitialize();
					}
				})}
			{:else if snapshot.service.phase === 'paused'}
				{@render button({
					kind: 'resume',
					label: text.resume,
					disabled: snapshot.disposing || snapshot.pending !== null || active,
					onclick: () => {
						void controller.resume();
					}
				})}
			{/if}
			{#if snapshot.service.error}<details>
					<summary>{text.details}</summary>
					<pre class="overflow-auto text-xs whitespace-pre-wrap">{snapshot.service.error
							.code}: {snapshot.service.error.message}</pre>
				</details>{/if}
		</div>
	{/if}
	{#if snapshot.service?.catalog.freshness === 'cached'}<p class="text-muted-foreground text-sm">
			{text.cachedCatalog}
		</p>{/if}
	{#if snapshot.service?.catalog.error}
		<div role="status">
			<p>{text.catalogFailed}</p>
			<details>
				<summary>{text.details}</summary>
				<pre class="overflow-auto text-xs whitespace-pre-wrap">{snapshot.service.catalog.error
						.message}</pre>
			</details>
		</div>
	{/if}
	{#if snapshot.service && snapshot.service.recovery.warnings.length > 0}
		<details>
			<summary>{text.recoveryWarnings}</summary>
			<ul>
				{#each snapshot.service.recovery.warnings as warning, index (index)}<li>
						{warning}
					</li>{/each}
			</ul>
		</details>
	{/if}
	{#each snapshot.errors as issue (issue.id)}
		<div
			class="border-destructive grid gap-2 rounded-md border p-3"
			data-model-error={issue.error.code}
			role="alert"
		>
			<p>{issue.error.message}</p>
			<details>
				<summary>{text.details}</summary>
				<pre
					class="overflow-auto text-xs whitespace-pre-wrap">{`${issue.error.code}\n${JSON.stringify(issue.error.details, null, 2)}`}</pre>
			</details>
			<button
				type="button"
				class="justify-self-start underline"
				aria-label={text.dismissError}
				onclick={() => controller.dismissError(issue.id)}>{text.dismissError}</button
			>
		</div>
	{/each}
	{#if snapshot.notice}<p class="text-sm" role="status" data-model-notice={snapshot.notice.kind}>
			{text.pair(snapshot.notice.pair, text.language)}: {text.notice(snapshot.notice.kind)}
		</p>{/if}

	{#if snapshot.installation}
		{@const operation = snapshot.installation}
		{#if progress}
			{@render progress(operation, cancelControl())}
		{:else}
			<div class="grid gap-2 rounded-md border p-3" data-model-installation={operation.id}>
				<p class="text-sm font-medium">{text.pair(operation.model.pair, text.language)}</p>
				<p role="status">{installationLabel(operation)}</p>
				<progress
					class="h-2 w-full"
					aria-label={text.progressLabel}
					value={modelInstallationProgress(operation)}
					max="100"
				></progress>
				{#if cancelControl()}{@render button(cancelControl()!)}
					<p class="text-muted-foreground text-xs">{text.cancelHelp}</p>{/if}
				<details data-model-progress-details>
					<summary>{text.details}</summary>
					<dl class="grid gap-1 text-xs">
						<dt>{text.modelId}</dt>
						<dd>{operation.model.id}</dd>
						<dt>{text.filename}</dt>
						<dd>{operation.progress.filename ?? '—'}</dd>
						<dt>{text.file}</dt>
						<dd>{operation.progress.fileIndex} / {operation.progress.fileCount}</dd>
						<dt>{text.attempt}</dt>
						<dd>{operation.progress.attempt} / {operation.progress.maxAttempts}</dd>
						<dt>{text.fileDownload}</dt>
						<dd>
							{text.bytes(operation.progress.fileDownloadBytes)} / {text.bytes(
								operation.progress.fileDownloadTotal
							)}
						</dd>
						<dt>{text.fileInstalled}</dt>
						<dd>
							{text.bytes(operation.progress.fileInstalledBytes)} / {text.bytes(
								operation.progress.fileInstalledTotal
							)}
						</dd>
						<dt>{text.downloaded}</dt>
						<dd>
							{text.bytes(operation.progress.completedDownloadBytes)} / {text.bytes(
								operation.progress.downloadTotal
							)}
						</dd>
						<dt>{text.decompressed}</dt>
						<dd>
							{text.bytes(operation.progress.completedInstalledBytes)} / {text.bytes(
								operation.progress.installedTotal
							)}
						</dd>
						<dt>{text.reused}</dt>
						<dd>{text.bytes(operation.progress.reusedBytes)}</dd>
						<dt>{text.network}</dt>
						<dd>{text.bytes(operation.progress.networkBytes)}</dd>
					</dl>
					{#if operation.progress.detail}<pre
							class="overflow-auto text-xs whitespace-pre-wrap">{operation.progress
								.detail}</pre>{/if}
				</details>
				{#if operation.result && operation.result.cleanupWarnings.length > 0}<details>
						<summary>{text.cleanupWarnings}</summary>
						<ul>
							{#each operation.result.cleanupWarnings as warning, index (index)}<li>
									{warning}
								</li>{/each}
						</ul>
					</details>{/if}
			</div>
		{/if}
	{/if}

	{#each sections as section (section.id)}
		{#if section.id !== 'attention' || section.rows.length > 0}
			<section class="grid gap-2" data-model-section={section.id}>
				<h3 class="font-medium">{section.label}</h3>
				{#if section.rows.length === 0}<p class="text-muted-foreground text-sm">
						{section.empty}
					</p>{/if}
				{#each section.rows as row (row.key)}
					<div class="grid min-w-0 gap-2 rounded-md border p-3" data-model-row={row.key}>
						{#if modelRow}
							{@render modelRow(row, controls(row))}
						{:else}
							<p class="font-medium">
								{row.pair ? text.pair(row.pair, text.language) : row.local?.directory}
							</p>
							{#if row.local}<p class="text-muted-foreground text-sm">
									{text.localState(row.local.state)}{row.inUse ? ` · ${text.inUse}` : ''}
								</p>{/if}
							{#if row.model}<p class="text-muted-foreground text-xs">
									{text.version}: {row.model.version} · {text.downloadSize}: {text.bytes(
										row.model.downloadBytes
									)} · {text.installedSize}: {text.bytes(row.model.installedBytes)}
								</p>{/if}
							{#if row.local?.matchesCurrentCatalog === false}<p class="text-sm">
									{text.differentCatalog}
								</p>{/if}
							{#if row.local?.state === 'unmanaged'}<p class="text-sm">{text.unmanaged}</p>{/if}
							{#if row.local || row.model}<details>
									<summary class="text-sm">{text.details}</summary>{#if row.model}<p
											class="text-xs break-all"
										>
											{text.modelId}: {row.model.id}
										</p>{/if}{#if row.local}<p class="text-xs break-all">
											{text.directory}: {row.local.directory}
										</p>
										{#if row.local.detail}<pre
												class="overflow-auto text-xs whitespace-pre-wrap">{row.local
													.detail}</pre>{/if}{/if}
								</details>{/if}
							<div class="flex flex-wrap gap-2">
								{#each controls(row) as control (control.kind)}{@render button(control)}{/each}
							</div>
						{/if}
					</div>
				{/each}
			</section>
		{/if}
	{/each}
</section>

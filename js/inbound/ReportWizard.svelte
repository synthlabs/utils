<script lang="ts">
	import { untrack } from 'svelte';
	import Logger from '$utils/log';
	import { inboundCapabilities, previewBuild, previewConfig, previewError, previewLogTail, previewSystem, submitReport } from './client';
	import IncludeToggle from './IncludeToggle.svelte';
	import type { BuildInfo, ErrorContext, LogPreview, SystemInfo } from './types';
	import { InboundWizardState } from './wizard.svelte';

	interface Props {
		prefilledError?: ErrorContext | null;
		onclose?: () => void;
	}

	let { prefilledError = null, onclose = () => {} }: Props = $props();
	const initialError = untrack(() => prefilledError);
	const wizard = new InboundWizardState(initialError);

	let system = $state<SystemInfo | null>(null);
	let build = $state<BuildInfo | null>(null);
	let log = $state<LogPreview | null>(null);
	let config = $state<string | null>(null);
	let error = $state<ErrorContext | null>(initialError);
	let busy = $state(false);
	let progressSteps = $derived(wizard.visibleSteps());
	let progressIndex = $derived(progressSteps.indexOf(wizard.step));
	let progressPosition = $derived(progressIndex >= 0 ? progressIndex : progressSteps.length - 1);

	inboundCapabilities()
		.then((capabilities) => {
			wizard.hasConfig = capabilities.has_config;
			wizard.include.config = capabilities.has_config;
		})
		.catch(Logger.error);

	$effect(() => {
		if (wizard.step === 'system' && system === null) previewSystem().then((value) => (system = value)).catch(Logger.error);
		if (wizard.step === 'build' && build === null) previewBuild().then((value) => (build = value)).catch(Logger.error);
		if (wizard.step === 'log' && log === null) previewLogTail().then((value) => (log = value)).catch(Logger.error);
		if (wizard.step === 'config' && config === null) previewConfig().then((value) => (config = value)).catch(Logger.error);
		if (wizard.step === 'error' && error === null) previewError().then((value) => (error = value)).catch(Logger.error);
	});

	async function send() {
		busy = true;
		wizard.errorMessage = '';
		wizard.step = 'sending';
		try {
			await submitReport({
				discord_user: wizard.discordUser.trim() || null,
				message: wizard.message.trim(),
				include: wizard.include
			});
			wizard.step = 'done';
		} catch (e) {
			Logger.error('inbound submit failed', e);
			wizard.errorMessage = String(e);
			wizard.step = 'failed';
		} finally {
			busy = false;
		}
	}

	function fmtBytes(value: number): string {
		if (value < 1024) return `${value} B`;
		if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
		return `${(value / (1024 * 1024)).toFixed(1)} MB`;
	}
</script>

<div class="flex min-h-[420px] flex-col gap-4">
	<div class="flex items-start justify-between gap-4">
		<div class="min-w-0">
			<h2 class="text-lg font-semibold">Send a bug report</h2>
			<p class="text-muted-foreground text-sm">Review each item before it leaves this machine.</p>
		</div>
	</div>
	<div class="flex items-center gap-3" aria-label={`Step ${progressPosition + 1} of ${progressSteps.length}`}>
		<div class="text-muted-foreground shrink-0 text-xs tabular-nums">Step {progressPosition + 1} of {progressSteps.length}</div>
		<div class="flex min-w-0 flex-1 gap-1" aria-hidden="true">
			{#each progressSteps as step, index}
				<div class="h-1.5 flex-1 rounded-full transition-colors" class:bg-primary={index <= progressPosition} class:bg-muted={index > progressPosition}></div>
			{/each}
		</div>
	</div>

	{#if wizard.step === 'username'}
		<section class="flex flex-1 flex-col gap-3">
			<label class="text-sm font-medium" for="inbound-discord">Discord username</label>
			<input id="inbound-discord" class="border-input bg-background focus-visible:ring-ring focus-visible:ring-offset-background h-9 rounded-md border px-3 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-hidden" bind:value={wizard.discordUser} placeholder="optional" autocomplete="off" autocapitalize="none" spellcheck={false} />
			<p class="text-muted-foreground text-xs">This is only used to follow up about this report.</p>
		</section>
	{:else if wizard.step === 'message'}
		<section class="flex flex-1 flex-col gap-3">
			<label class="text-sm font-medium" for="inbound-message">What happened? <span class="text-muted-foreground font-normal">(optional)</span></label>
			<textarea id="inbound-message" class="border-input bg-background min-h-32 resize-none rounded-md border p-3 text-sm" bind:value={wizard.message} placeholder="Add any context, expected behavior, or steps you tried."></textarea>
		</section>
	{:else if wizard.step === 'system'}
		<section class="flex flex-1 flex-col gap-3">
			<IncludeToggle bind:checked={wizard.include.system} title="System info" description="Operating system, CPU architecture, and locale." />
			<pre class="bg-muted/50 max-h-48 overflow-auto rounded border p-3 text-xs">{JSON.stringify(system, null, 2)}</pre>
		</section>
	{:else if wizard.step === 'build'}
		<section class="flex flex-1 flex-col gap-3">
			<IncludeToggle bind:checked={wizard.include.build} title="Build info" description="App version, git commit, and build timestamp." />
			<pre class="bg-muted/50 max-h-48 overflow-auto rounded border p-3 text-xs">{JSON.stringify(build, null, 2)}</pre>
		</section>
	{:else if wizard.step === 'log'}
		<section class="flex flex-1 flex-col gap-3">
			<IncludeToggle bind:checked={wizard.include.log} title="Log tail" description="The most recent local app log lines, compressed before sending." />
			{#if log?.exists}
				<div class="text-muted-foreground text-xs">{fmtBytes(log.byte_count)} raw, {fmtBytes(log.gzipped_size)} compressed</div>
				<pre class="bg-muted/50 max-h-48 overflow-auto whitespace-pre-wrap rounded border p-3 text-xs">{log.preview_text}</pre>
			{:else}
				<div class="text-muted-foreground rounded border p-3 text-sm">No log file was found.</div>
			{/if}
		</section>
	{:else if wizard.step === 'config'}
		<section class="flex flex-1 flex-col gap-3">
			<IncludeToggle bind:checked={wizard.include.config} title="Configuration snapshot" description="App settings with known secrets redacted." />
			<pre class="bg-muted/50 max-h-48 overflow-auto rounded border p-3 text-xs">{config ?? ''}</pre>
		</section>
	{:else if wizard.step === 'error'}
		<section class="flex flex-1 flex-col gap-3">
			<IncludeToggle bind:checked={wizard.include.error} title="Detected error" description="The first captured error or panic from this session." />
			<pre class="bg-muted/50 max-h-48 overflow-auto whitespace-pre-wrap rounded border p-3 text-xs">{JSON.stringify(error, null, 2)}</pre>
		</section>
	{:else if wizard.step === 'preview'}
		<section class="flex flex-1 flex-col gap-3">
			<h3 class="text-sm font-medium">Preview</h3>
			<div class="grid gap-2 text-sm">
				<div><span class="text-muted-foreground">Discord:</span> {wizard.discordUser || '(not provided)'}</div>
				<div><span class="text-muted-foreground">Message:</span> {wizard.message || '(not provided)'}</div>
				<div><span class="text-muted-foreground">Included:</span> {Object.entries(wizard.include).filter(([, v]) => v).map(([k]) => k).join(', ') || 'none'}</div>
			</div>
		</section>
	{:else if wizard.step === 'sending'}
		<section class="grid flex-1 place-items-center text-sm text-muted-foreground">Sending report...</section>
	{:else if wizard.step === 'done'}
		<section class="grid flex-1 place-items-center gap-3 text-center">
			<div>
				<h3 class="font-semibold">Report sent</h3>
				<p class="text-muted-foreground text-sm">Thanks. A private Discord thread was created for this report.</p>
			</div>
		</section>
	{:else if wizard.step === 'failed'}
		<section class="grid flex-1 place-items-center gap-3 text-center">
			<div>
				<h3 class="font-semibold">Report failed</h3>
				<p class="text-muted-foreground text-sm">{wizard.errorMessage}</p>
			</div>
		</section>
	{/if}

	<div class="flex items-center justify-between gap-2 border-t pt-3">
		<button type="button" class="border-input hover:bg-muted h-9 rounded-md border px-3 text-sm disabled:opacity-40" disabled={wizard.step === 'username' || wizard.step === 'sending' || wizard.step === 'done'} onclick={() => wizard.back()}>
			Back
		</button>
		{#if wizard.step === 'preview'}
			<button type="button" class="bg-primary text-primary-foreground h-9 cursor-pointer rounded-md px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40" disabled={busy} onclick={send}>Send</button>
		{:else if wizard.step === 'done'}
			<button type="button" class="bg-primary text-primary-foreground h-9 cursor-pointer rounded-md px-3 text-sm" onclick={onclose}>Done</button>
		{:else if wizard.step === 'failed'}
			<button type="button" class="bg-primary text-primary-foreground h-9 cursor-pointer rounded-md px-3 text-sm" onclick={() => (wizard.step = 'preview')}>Try again</button>
		{:else}
			<button type="button" class="bg-primary text-primary-foreground h-9 cursor-pointer rounded-md px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40" onclick={() => wizard.next()}>Next</button>
		{/if}
	</div>
</div>

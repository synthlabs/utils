<script lang="ts">
	import { onDestroy, onMount, untrack, type Snippet } from 'svelte';
	import Logger from '$utils/log';
	import {
		inboundCapabilities,
		previewBuild,
		previewConfig,
		previewError,
		previewLogTail,
		previewSystem,
		submitReport
	} from './client';
	import IncludeToggle from './IncludeToggle.svelte';
	import { errorDetails } from './controller';
	import {
		defaultWizardCopy,
		type WizardCopy,
		type WizardAction,
		type WizardInclude
	} from './copy';
	import type { BuildInfo, ErrorContext, LogPreview, SystemInfo, StepId } from './types';
	import { InboundWizardState } from './wizard.svelte';

	let {
		prefilledError = null,
		onclose = () => {},
		submitSnapshot = false,
		onerror = (error: unknown) => Logger.error('inbound report failed', error),
		copy = {},
		class: className = '',
		header,
		action,
		includeToggle
	}: {
		prefilledError?: ErrorContext | null;
		onclose?: () => void;
		submitSnapshot?: boolean;
		onerror?: (error: unknown) => void;
		copy?: Partial<WizardCopy>;
		class?: string;
		header?: Snippet;
		action?: Snippet<[WizardAction]>;
		includeToggle?: Snippet<[WizardInclude]>;
	} = $props();
	const initialError = untrack(() => (prefilledError ? { ...prefilledError } : null));
	const wizard = new InboundWizardState(initialError);
	const text = $derived({ ...defaultWizardCopy, ...copy });
	type PreviewStep = 'system' | 'build' | 'log' | 'config' | 'error';
	const isPreview = (step: StepId): step is PreviewStep =>
		['system', 'build', 'log', 'config', 'error'].includes(step);
	const loaded = new Set<PreviewStep>(initialError ? ['error'] : []);
	let system = $state<SystemInfo | null>(null);
	let build = $state<BuildInfo | null>(null);
	let log = $state<LogPreview | null>(null);
	let config = $state<string | null>(null);
	let error = $state<ErrorContext | null>(initialError);
	let loading = $state(true);
	let busy = $state(false);
	let failure = $state<{
		step: PreviewStep | 'capabilities';
		message: string;
	} | null>(null);
	let disposed = false;
	let request = 0;
	const progressSteps = $derived(wizard.visibleSteps());
	const progressIndex = $derived(progressSteps.indexOf(wizard.step));
	const progressPosition = $derived(
		progressIndex >= 0 ? progressIndex : progressSteps.length - 1
	);

	async function load(step: PreviewStep | 'capabilities') {
		const current = ++request;
		loading = true;
		failure = null;
		try {
			switch (step) {
				case 'capabilities': {
					const value = await inboundCapabilities();
					if (disposed || current !== request) return;
					wizard.hasConfig = value.has_config;
					wizard.include.config = value.has_config;
					break;
				}
				case 'system':
					system = await previewSystem();
					break;
				case 'build':
					build = await previewBuild();
					break;
				case 'log':
					log = await previewLogTail();
					break;
				case 'config':
					config = await previewConfig();
					break;
				case 'error':
					error = await previewError();
					break;
			}
			if (step !== 'capabilities') loaded.add(step);
		} catch (error) {
			if (disposed || current !== request) return;
			onerror(error);
			failure = { step, message: errorDetails(error) };
		} finally {
			if (!disposed && current === request) loading = false;
		}
	}

	onMount(() => {
		void load('capabilities');
	});
	onDestroy(() => {
		disposed = true;
		request++;
	});
	$effect(() => {
		const step = wizard.step;
		untrack(() => {
			failure = null;
			if (isPreview(step) && !loaded.has(step)) void load(step);
		});
	});

	async function send() {
		if (busy) return;
		busy = true;
		wizard.errorMessage = '';
		wizard.step = 'sending';
		try {
			await submitReport({
				discord_user: wizard.discordUser.trim() || null,
				message: wizard.message.trim(),
				include: { ...wizard.include },
				...(submitSnapshot ? { error: initialError } : {})
			});
			if (!disposed) wizard.step = 'done';
		} catch (error) {
			onerror(error);
			if (!disposed) {
				wizard.errorMessage = errorDetails(error);
				wizard.step = 'failed';
			}
		} finally {
			busy = false;
		}
	}

	function fmtBytes(value: number): string {
		if (value < 1024) return `${value} B`;
		if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
		return `${(value / (1024 * 1024)).toFixed(1)} MB`;
	}
	const previewText = $derived(
		wizard.step === 'system'
			? JSON.stringify(system, null, 2)
			: wizard.step === 'build'
				? JSON.stringify(build, null, 2)
				: wizard.step === 'log'
					? (log?.preview_text ?? '')
					: wizard.step === 'config'
						? (config ?? '')
						: JSON.stringify(error, null, 2)
	);
	const nextAction = $derived<WizardAction>(
		wizard.step === 'preview'
			? { kind: 'send', label: text.send, disabled: busy, onclick: send }
			: wizard.step === 'done'
				? {
						kind: 'close',
						label: text.close,
						disabled: false,
						onclick: onclose
					}
				: wizard.step === 'failed'
					? {
							kind: 'retry',
							label: text.retry,
							disabled: false,
							onclick: () => (wizard.step = 'preview')
						}
					: {
							kind: 'next',
							label: text.next,
							disabled:
								busy ||
								loading ||
								(!!failure &&
									(failure.step === 'capabilities' ||
										wizard.include[failure.step])),
							onclick: () => wizard.next()
						}
	);
</script>

{#snippet button(props: WizardAction)}
	{#if action}{@render action(props)}{:else}
		<button
			type="button"
			data-inbound-action={props.kind}
			class="h-9 cursor-pointer rounded-md px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
			class:border={props.kind === 'back'}
			class:border-input={props.kind === 'back'}
			class:hover:bg-muted={props.kind === 'back'}
			class:bg-primary={props.kind !== 'back'}
			class:text-primary-foreground={props.kind !== 'back'}
			disabled={props.disabled}
			onclick={props.onclick}>{props.label}</button
		>
	{/if}
{/snippet}

<div
	class={`flex min-h-[420px] min-w-0 flex-col gap-4 ${className}`}
	data-inbound-wizard
	data-step={wizard.step}
>
	<div class="min-w-0 pr-8">
		{#if header}{@render header()}{:else}
			<h2 class="text-lg font-semibold">{text.title}</h2>
			<p class="text-muted-foreground text-sm">{text.description}</p>
		{/if}
	</div>
	<div class="flex items-center gap-3">
		<p class="text-muted-foreground shrink-0 text-xs tabular-nums" role="status">
			{text.step(progressPosition + 1, progressSteps.length)}
		</p>
		<div class="flex min-w-0 flex-1 gap-1" aria-hidden="true">
			{#each progressSteps as step, index (step)}
				<div
					class="h-1.5 flex-1 rounded-full transition-colors"
					class:bg-primary={index <= progressPosition}
					class:bg-muted={index > progressPosition}
				></div>
			{/each}
		</div>
	</div>

	{#if failure}
		<section class="grid gap-3" data-inbound-load-error>
			<p class="text-destructive text-sm" role="alert">
				{text.loadFailed}
			</p>
			<details>
				<summary>{text.technicalDetails}</summary>
				<pre data-inbound-preview>{failure.message}</pre>
			</details>
			{@render button({
				kind: 'retry',
				label: text.retry,
				disabled: loading,
				onclick: () => {
					if (failure) void load(failure.step);
				}
			})}
		</section>
	{:else if loading}
		<p class="text-muted-foreground text-sm" role="status">
			{text.loading}
		</p>
	{/if}

	{#if wizard.step === 'username'}
		<section class="flex flex-1 flex-col gap-3">
			<label class="text-sm font-medium" for="inbound-discord">{text.username}</label>
			<input
				id="inbound-discord"
				class="border-input bg-background focus-visible:ring-ring focus-visible:ring-offset-background h-9 rounded-md border px-3 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-hidden"
				bind:value={wizard.discordUser}
				placeholder={text.optional}
				autocomplete="off"
				autocapitalize="none"
				spellcheck={false}
			/>
			<p class="text-muted-foreground text-xs">{text.usernameHelp}</p>
		</section>
	{:else if wizard.step === 'message'}
		<section class="flex flex-1 flex-col gap-3">
			<label class="text-sm font-medium" for="inbound-message"
				>{text.message}
				<span class="text-muted-foreground font-normal">({text.optional})</span></label
			>
			<textarea
				id="inbound-message"
				class="border-input bg-background min-h-32 resize-none rounded-md border p-3 text-sm"
				bind:value={wizard.message}
				placeholder={text.messagePlaceholder}
			></textarea>
		</section>
	{:else if isPreview(wizard.step)}
		{@const step = wizard.step}
		<section class="flex min-h-0 min-w-0 flex-1 flex-col gap-3">
			{#if includeToggle}
				{@render includeToggle({
					id: `inbound-include-${step}`,
					title: text[step],
					description: text[`${step}Help`],
					checked: wizard.include[step],
					onchange: (value) => (wizard.include[step] = value)
				})}
			{:else}
				<IncludeToggle
					bind:checked={wizard.include[step]}
					title={text[step]}
					description={text[`${step}Help`]}
				/>
			{/if}
			{#if !loading && !failure}
				{#if step === 'log' && !log?.exists}
					<p class="text-muted-foreground rounded border p-3 text-sm">
						{text.noLog}
					</p>
				{:else}
					{#if step === 'log' && log}
						<p class="text-muted-foreground text-xs">
							{text.logSize(fmtBytes(log.byte_count), fmtBytes(log.gzipped_size))}
						</p>
					{/if}
					<pre
						class="bg-muted/50 max-h-48 overflow-auto rounded border p-3 text-xs whitespace-pre-wrap"
						data-inbound-preview>{previewText}</pre>
				{/if}
			{/if}
		</section>
	{:else if wizard.step === 'preview'}
		<section class="flex flex-1 flex-col gap-3">
			<h3 class="text-sm font-medium">{text.preview}</h3>
			<div class="grid gap-2 text-sm">
				<p>
					<span class="text-muted-foreground">{text.discordLabel}</span>
					{wizard.discordUser || text.notProvided}
				</p>
				<p>
					<span class="text-muted-foreground">{text.messageLabel}</span>
					{wizard.message || text.notProvided}
				</p>
				<p>
					<span class="text-muted-foreground">{text.includedLabel}</span>
					{Object.entries(wizard.include)
						.filter(([, value]) => value)
						.map(([key]) => text[key as PreviewStep])
						.join(', ') || text.none}
				</p>
			</div>
		</section>
	{:else if wizard.step === 'sending'}
		<p class="text-muted-foreground grid flex-1 place-items-center text-sm" role="status">
			{text.sending}
		</p>
	{:else if wizard.step === 'done'}
		<section class="grid flex-1 place-items-center gap-3 text-center" role="status">
			<div>
				<h3 class="font-semibold">{text.done}</h3>
				<p class="text-muted-foreground text-sm">{text.doneHelp}</p>
			</div>
		</section>
	{:else if wizard.step === 'failed'}
		<section class="grid flex-1 gap-3">
			<h3 class="text-destructive font-semibold" role="alert">
				{text.failed}
			</h3>
			<details>
				<summary>{text.technicalDetails}</summary>
				<pre data-inbound-preview>{wizard.errorMessage}</pre>
			</details>
		</section>
	{/if}

	<div class="flex items-center justify-between gap-2 border-t pt-3" data-inbound-footer>
		{@render button({
			kind: 'back',
			label: text.back,
			disabled:
				loading || busy || ['username', 'sending', 'done', 'failed'].includes(wizard.step),
			onclick: () => wizard.back()
		})}
		{@render button(nextAction)}
	</div>
</div>

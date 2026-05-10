<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import X from '@lucide/svelte/icons/x';
	import { onDestroy, onMount } from 'svelte';
	import ReportWizard from './ReportWizard.svelte';
	import type { ErrorContext } from './types';

	let visible = $state(false);
	let wizardOpen = $state(false);
	let error = $state<ErrorContext | null>(null);
	let unlisten: UnlistenFn | null = null;

	onMount(async () => {
		unlisten = await listen<ErrorContext>('inbound://error-detected', (event) => {
			error = event.payload;
			visible = true;
		});
	});

	onDestroy(() => {
		unlisten?.();
	});
</script>

{#if visible && !wizardOpen}
	<div class="fixed right-4 bottom-4 z-[100] w-[min(24rem,calc(100vw-2rem))] rounded-md border bg-background p-4 shadow-lg">
		<div class="flex flex-col gap-3">
			<div>
				<div class="text-sm font-semibold">Error detected</div>
				<div class="text-muted-foreground line-clamp-2 text-xs">{error?.message}</div>
			</div>
			<div class="flex justify-end gap-2">
				<button type="button" class="border-input hover:bg-muted h-8 cursor-pointer rounded-md border px-3 text-xs" onclick={() => (visible = false)}>Dismiss</button>
				<button type="button" class="bg-primary text-primary-foreground h-8 cursor-pointer rounded-md px-3 text-xs" onclick={() => (wizardOpen = true)}>Report this</button>
			</div>
		</div>
	</div>
{/if}

{#if wizardOpen}
	<div class="fixed inset-0 z-[110] grid place-items-center bg-black/40 p-4">
		<div class="bg-background relative w-[min(38rem,100%)] rounded-lg border p-4 shadow-xl">
			<button type="button" class="text-muted-foreground hover:text-foreground absolute top-4 right-4 inline-flex size-8 cursor-pointer items-center justify-center rounded-sm transition-colors" aria-label="Close bug report" onclick={() => {
				wizardOpen = false;
				visible = false;
			}}>
				<X class="size-4" />
			</button>
			<ReportWizard prefilledError={error} onclose={() => {
				wizardOpen = false;
				visible = false;
			}} />
		</div>
	</div>
{/if}

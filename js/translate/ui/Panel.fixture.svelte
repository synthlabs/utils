<script lang="ts">
	import type { ModelManagerController } from '../controller';
	import ModelManagerPanel from './ModelManagerPanel.svelte';
	let { controller }: { controller: ModelManagerController } = $props();
</script>

<ModelManagerPanel
	{controller}
	class="custom-panel"
	copy={{ language: (code) => code.toUpperCase() }}
>
	{#snippet header(snapshot)}<h2 data-custom-header>
			Custom models: {snapshot.installed.length}
		</h2>{/snippet}
	{#snippet action(control)}<button
			type="button"
			data-custom-action={control.kind}
			disabled={control.disabled}
			onclick={control.onclick}>{control.label}</button
		>{/snippet}
	{#snippet modelRow(row, controls)}
		<div data-custom-row>{row.pair?.source} / {row.pair?.target}</div>
		{#each controls as control (control.kind)}<button
				type="button"
				data-custom-row-action={control.kind}
				disabled={control.disabled}
				onclick={control.onclick}>{control.label}</button
			>{/each}
	{/snippet}
	{#snippet progress(operation, cancel)}
		<p data-custom-progress>{operation.progress.phase}</p>
		{#if cancel}<button
				type="button"
				data-custom-cancel
				disabled={cancel.disabled}
				onclick={cancel.onclick}>{cancel.label}</button
			>{/if}
	{/snippet}
</ModelManagerPanel>

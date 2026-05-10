<script lang="ts">
	interface Props {
		checked: boolean;
		title: string;
		description: string;
	}

	let { checked = $bindable(), title, description }: Props = $props();
</script>

<label class="flex items-start gap-3 rounded border p-3">
	<input type="checkbox" class="mt-1" bind:checked />
	<span class="grid gap-1">
		<span class="text-sm font-medium">{title}</span>
		<span class="text-muted-foreground text-xs">{description}</span>
	</span>
</label>

<script lang="ts" generics="L extends string">
	import { DropdownMenu } from 'bits-ui';
	import type { LanguageController } from './languageController.svelte.ts';

	interface Props {
		controller: LanguageController<L>;
		labels?: Record<string, string>;
		align?: 'start' | 'end' | 'center';
		accentColor?: string;
	}

	let { controller, labels = {}, align = 'end', accentColor }: Props = $props();

	function labelFor(locale: string): string {
		return labels[locale] ?? locale.toUpperCase();
	}
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<button {...props} type="button" class="ls-trigger">
				<svg
					class="ls-icon"
					viewBox="0 0 24 24"
					width="14"
					height="14"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<circle cx="12" cy="12" r="10" />
					<path d="M2 12h20" />
					<path
						d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"
					/>
				</svg>
				<span class="ls-label">{labelFor(controller.current)}</span>
				<svg
					class="ls-caret"
					viewBox="0 0 24 24"
					width="12"
					height="12"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					stroke-linecap="round"
					stroke-linejoin="round"
					aria-hidden="true"
				>
					<polyline points="6 9 12 15 18 9" />
				</svg>
			</button>
		{/snippet}
	</DropdownMenu.Trigger>

	<DropdownMenu.Content class="ls-content" {align} sideOffset={6}>
		{#each controller.locales as locale (locale)}
			{@const active = locale === controller.current}
			<DropdownMenu.Item onclick={() => controller.set(locale)}>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						class="ls-item"
						class:active
						style:--ls-accent-color={active ? accentColor : undefined}
					>
						<span class="ls-code">{locale.toUpperCase()}</span>
						<span class="ls-name">{labelFor(locale)}</span>
						{#if active}
							<svg
								class="ls-check"
								viewBox="0 0 24 24"
								width="14"
								height="14"
								fill="none"
								stroke="currentColor"
								stroke-width="2.5"
								stroke-linecap="round"
								stroke-linejoin="round"
								aria-hidden="true"
							>
								<polyline points="20 6 9 17 4 12" />
							</svg>
						{/if}
					</button>
				{/snippet}
			</DropdownMenu.Item>
		{/each}
	</DropdownMenu.Content>
</DropdownMenu.Root>

<style>
	.ls-trigger {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		background: transparent;
		border: 1px solid hsl(var(--border));
		border-radius: var(--radius);
		padding: 5px 8px 5px 9px;
		color: hsl(var(--muted-foreground));
		cursor: pointer;
		font-family: inherit;
		font-size: 13px;
		line-height: 1;
		transition:
			color 150ms ease,
			border-color 150ms ease,
			background-color 150ms ease;
	}
	.ls-trigger:hover {
		color: hsl(var(--foreground));
		background: hsl(var(--accent));
	}
	.ls-trigger:focus-visible {
		outline: 2px solid hsl(var(--ring, var(--accent)));
		outline-offset: 2px;
	}
	.ls-icon,
	.ls-caret {
		flex-shrink: 0;
	}
	.ls-caret {
		opacity: 0.7;
		margin-left: 1px;
	}
	.ls-label {
		font-size: 13px;
		letter-spacing: 0.01em;
	}

	:global(.ls-content) {
		min-width: 180px;
		background: hsl(var(--popover));
		color: hsl(var(--popover-foreground));
		border: 1px solid hsl(var(--border));
		border-radius: var(--radius);
		padding: 4px;
		box-shadow:
			0 10px 30px -12px rgb(0 0 0 / 0.45),
			0 0 0 1px rgb(0 0 0 / 0.05);
		z-index: 50;
	}

	.ls-item {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		padding: 7px 8px 7px 10px;
		border-radius: calc(var(--radius) - 2px);
		background: transparent;
		border: none;
		font-family: inherit;
		font-size: 13px;
		color: hsl(var(--foreground));
		cursor: pointer;
		text-align: left;
	}
	.ls-item:hover,
	.ls-item:focus-visible {
		background: hsl(var(--accent));
		outline: none;
	}
	.ls-code {
		font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
		font-size: 11px;
		letter-spacing: 0.04em;
		color: hsl(var(--muted-foreground));
		min-width: 22px;
	}
	.ls-item.active .ls-code {
		color: var(--ls-accent-color, hsl(var(--foreground)));
	}
	.ls-name {
		flex: 1;
		font-size: 13px;
	}
	.ls-check {
		flex-shrink: 0;
		opacity: 0.85;
	}
</style>

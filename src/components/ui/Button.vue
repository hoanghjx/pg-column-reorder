<script setup lang="ts">
import { cn } from '@/lib/utils'

interface Props {
  variant?: 'default' | 'outline' | 'ghost' | 'destructive' | 'secondary'
  size?: 'default' | 'sm' | 'icon'
  class?: string
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'default',
  size: 'default',
  class: '',
  type: 'button',
})
</script>

<template>
  <button
    :type="props.type"
    :disabled="props.disabled"
    :class="cn(
      'inline-flex shrink-0 items-center justify-center gap-2 rounded-md text-sm font-medium transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50',
      props.variant === 'default' && 'bg-primary text-primary-foreground hover:brightness-95',
      props.variant === 'outline' && 'border bg-background hover:bg-accent hover:text-accent-foreground',
      props.variant === 'ghost' && 'hover:bg-accent hover:text-accent-foreground',
      props.variant === 'destructive' && 'bg-destructive text-destructive-foreground hover:brightness-95',
      props.variant === 'secondary' && 'bg-secondary text-secondary-foreground hover:bg-secondary/80',
      props.size === 'default' && 'h-9 px-4 py-2',
      props.size === 'sm' && 'h-8 px-3 text-xs',
      props.size === 'icon' && 'size-9',
      props.class,
    )"
  >
    <slot />
  </button>
</template>

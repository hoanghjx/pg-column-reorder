<script setup lang="ts">
import { cn } from '@/lib/utils'

interface Props {
  variant?: 'default' | 'secondary' | 'destructive' | 'outline'
  class?: string
}

const props = withDefaults(defineProps<Props>(), { variant: 'default', class: '' })
</script>

<template>
  <span :class="cn('inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium', props.variant === 'default' && 'border-transparent bg-primary text-primary-foreground', props.variant === 'secondary' && 'border-transparent bg-secondary text-secondary-foreground', props.variant === 'destructive' && 'border-transparent bg-destructive text-destructive-foreground', props.variant === 'outline' && 'text-foreground', props.class)">
    <slot />
  </span>
</template>

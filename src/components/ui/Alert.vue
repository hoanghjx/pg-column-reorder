<script setup lang="ts">
import { cn } from '@/lib/utils'

interface Props {
  variant?: 'default' | 'destructive' | 'success' | 'warning'
  class?: string
}

const props = withDefaults(defineProps<Props>(), { variant: 'default', class: '' })
</script>

<template>
  <div role="alert" :class="cn('rounded-md border px-3 py-2 text-sm', props.variant === 'default' && 'bg-card', props.variant === 'destructive' && 'border-destructive/30 bg-destructive/8 text-destructive', props.variant === 'success' && 'border-emerald-600/25 bg-emerald-600/8 text-emerald-800', props.variant === 'warning' && 'border-amber-600/25 bg-amber-500/10 text-amber-900', props.class)">
    <slot />
  </div>
</template>

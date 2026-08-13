<script setup lang="ts">
import { cn } from '@/lib/utils'

interface Props {
  modelValue?: string
  type?: string
  placeholder?: string
  class?: string
  disabled?: boolean
  autocomplete?: string
}

const props = withDefaults(defineProps<Props>(), { type: 'text', modelValue: '', placeholder: '', class: '', autocomplete: '' })
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
</script>

<template>
  <input
    :value="props.modelValue"
    :type="props.type"
    :placeholder="props.placeholder"
    :disabled="props.disabled"
    :autocomplete="props.autocomplete"
    :class="cn('flex h-9 w-full rounded-md border bg-background px-3 py-1 text-sm shadow-sm outline-none transition focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50', props.class)"
    @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
  />
</template>

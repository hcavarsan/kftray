import { selectStyles } from '@/components/ui/select-styles'

import { Field, FieldCreatableSelect } from './Field'
import type { StringOption } from './types'
import { duplicateTagKey, optionsToTags, tagsToOptions } from './utils'

interface TagsFieldProps {
  error?: string
  onChange: (tags: Record<string, string>) => void
  onDuplicate: (message: string | null) => void
  options: StringOption[]
  tags?: Record<string, string>
}

export function TagsField({
  error,
  onChange,
  onDuplicate,
  options,
  tags,
}: TagsFieldProps) {
  return (
    <Field error={error} label='Tags'>
      <FieldCreatableSelect<StringOption, true>
        formatCreateLabel={value => `Add "${value}"`}
        isMulti
        onChange={values => {
          const duplicate = duplicateTagKey(values)
          if (duplicate) {
            onDuplicate(
              `Tag "${duplicate}" already has a value. Remove it first to change it.`,
            )
            return
          }
          onDuplicate(null)
          onChange(optionsToTags(values))
        }}
        options={options}
        placeholder='team=payments, env=dev, pinned'
        styles={selectStyles<StringOption, true>()}
        value={tagsToOptions(tags)}
      />
    </Field>
  )
}

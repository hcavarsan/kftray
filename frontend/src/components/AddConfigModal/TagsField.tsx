import CreatableSelect from 'react-select/creatable'

import { Text } from '@chakra-ui/react'

import { selectStyles } from '@/components/ui/select-styles'

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
    <>
      <Text fontSize='xs' color='gray.400'>
        Tags
      </Text>
      <CreatableSelect<StringOption, true>
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
      {error && (
        <Text color='red.300' fontSize='xs'>
          {error}
        </Text>
      )}
    </>
  )
}

import type { ReactNode } from 'react'
import Select, { type GroupBase, type Props as SelectProps } from 'react-select'
import CreatableSelect, { type CreatableProps } from 'react-select/creatable'

import {
  Field as ChakraField,
  Flex,
  Input,
  Stack,
  Text,
  useFieldContext,
} from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { selectStyles } from '@/components/ui/select-styles'

import type { ConfigDraft, StringOption } from './types'

const protocolOptions: StringOption[] = [
  { value: 'tcp', label: 'TCP' },
  { value: 'udp', label: 'UDP' },
]

interface DraftFieldProps {
  draft: ConfigDraft
  error?: string
  onUpdate: (update: Partial<ConfigDraft>) => void
}

interface FieldProps {
  children?: ReactNode
  error?: string
  hint?: ReactNode
  label: ReactNode
  'data-testid'?: string
}

interface TextFieldProps extends FieldProps {
  disabled?: boolean
  name: string
  onChange: (value: string) => void
  placeholder?: string
  type?: 'number' | 'text'
  value: string
}

export function Field({
  children,
  error,
  hint,
  label,
  'data-testid': testId,
}: FieldProps) {
  return (
    <ChakraField.Root data-testid={testId} gap={1.5} alignItems='stretch'>
      <Flex align='center' gap={1}>
        <ChakraField.Label
          fontSize='xs'
          fontWeight='normal'
          color='fg.muted'
          m={0}
        >
          {label}
        </ChakraField.Label>
        {hint}
      </Flex>
      {children}
      {error && (
        <Text color='danger.fg' fontSize='xs'>
          {error}
        </Text>
      )}
    </ChakraField.Root>
  )
}

export function FieldSelect<Option, IsMulti extends boolean = false>(
  props: SelectProps<Option, IsMulti, GroupBase<Option>>,
) {
  const field = useFieldContext()
  return <Select<Option, IsMulti> inputId={field?.ids.control} {...props} />
}

export function FieldCreatableSelect<Option, IsMulti extends boolean = false>(
  props: CreatableProps<Option, IsMulti, GroupBase<Option>>,
) {
  const field = useFieldContext()
  return (
    <CreatableSelect<Option, IsMulti> inputId={field?.ids.control} {...props} />
  )
}

export function TextField({
  disabled,
  error,
  hint,
  label,
  name,
  onChange,
  placeholder,
  type = 'text',
  value,
  'data-testid': testId,
}: TextFieldProps) {
  return (
    <Field data-testid={testId} error={error} hint={hint} label={label}>
      <Input
        bg='bg.surface'
        border='1px solid'
        borderColor='border'
        disabled={disabled}
        fontSize='13px'
        height='28px'
        name={name}
        onChange={event => onChange(event.target.value)}
        placeholder={placeholder}
        type={type}
        value={value}
        _focus={{ borderColor: 'accent.focusRing', boxShadow: 'none' }}
        _hover={{ borderColor: 'border.emphasized' }}
      />
    </Field>
  )
}

export function ProtocolField({ draft, error, onUpdate }: DraftFieldProps) {
  return (
    <Field data-testid='config-protocol' error={error} label='Protocol *'>
      <FieldSelect<StringOption>
        onChange={option =>
          onUpdate({ protocol: option?.value === 'udp' ? 'udp' : 'tcp' })
        }
        options={protocolOptions}
        styles={selectStyles<StringOption>()}
        value={
          protocolOptions.find(option => option.value === draft.protocol) ??
          null
        }
      />
    </Field>
  )
}

export function LocalAddressField({ draft, onUpdate }: DraftFieldProps) {
  return (
    <Stack gap={1.5}>
      <TextField
        disabled={draft.auto_loopback_address}
        label='Local Address (Optional)'
        name='local_address'
        onChange={local_address => onUpdate({ local_address })}
        placeholder={
          draft.auto_loopback_address ? '127.0.0.x' : 'e.g., 127.0.0.1'
        }
        value={
          draft.auto_loopback_address
            ? ''
            : (draft.local_address ?? '127.0.0.1')
        }
      />
      <Checkbox
        checked={draft.auto_loopback_address ?? false}
        onCheckedChange={event =>
          onUpdate({
            auto_loopback_address: event.checked === true,
            local_address: event.checked === true ? '' : '127.0.0.1',
          })
        }
        size='xs'
      >
        <Text fontSize='xs' color='fg.muted'>
          Auto select address
        </Text>
      </Checkbox>
    </Stack>
  )
}

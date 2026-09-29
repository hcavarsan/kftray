import type { ReactNode } from 'react'

import { Flex, Input, Stack, Text } from '@chakra-ui/react'

interface FieldProps {
  children?: ReactNode
  error?: string
  hint?: ReactNode
  label: ReactNode
}

interface TextFieldProps extends FieldProps {
  disabled?: boolean
  name: string
  onChange: (value: string) => void
  placeholder?: string
  type?: 'number' | 'text'
  value: string
}

export function Field({ children, error, hint, label }: FieldProps) {
  return (
    <Stack gap={1.5}>
      <Flex align='center' gap={1}>
        <Text fontSize='xs' color='gray.400'>
          {label}
        </Text>
        {hint}
      </Flex>
      {children}
      {error && (
        <Text color='red.300' fontSize='xs'>
          {error}
        </Text>
      )}
    </Stack>
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
}: TextFieldProps) {
  return (
    <Field error={error} hint={hint} label={label}>
      <Input
        bg='app.panel'
        border='1px solid'
        borderColor='app.border'
        disabled={disabled}
        fontSize='13px'
        height='28px'
        name={name}
        onChange={event => onChange(event.target.value)}
        placeholder={placeholder}
        type={type}
        value={value}
        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
        _hover={{ borderColor: 'app.borderStrong' }}
      />
    </Field>
  )
}

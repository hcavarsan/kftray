import type { ReactNode } from 'react'

import { Field, Flex, Input, Text } from '@chakra-ui/react'

import { cardProps } from './cardProps'

const numberInputProps = {
  type: 'number',
  size: 'xs',
  width: '60px',
  height: '24px',
  bg: 'app.bg',
  border: '1px solid',
  borderColor: 'app.border',
  _hover: { borderColor: 'app.borderStrong' },
  _focus: { borderColor: 'blue.400', boxShadow: 'none' },
  _invalid: { borderColor: 'red.400' },
  color: 'white',
  _placeholder: { color: 'whiteAlpha.500' },
  textAlign: 'center',
  fontSize: 'xs',
} as const

interface NumberFieldProps {
  label: string
  description: ReactNode
  value: string
  onChange: (value: string) => void
  placeholder: string
  min: number
  max: number
  unit: string
  error: string | undefined
}

export function NumberField({
  label,
  description,
  value,
  onChange,
  placeholder,
  min,
  max,
  unit,
  error,
}: NumberFieldProps) {
  return (
    <Field.Root {...cardProps} invalid={!!error}>
      <Flex direction='column' gap={2}>
        <Field.Label fontSize='sm' fontWeight='500' color='white'>
          {label}
        </Field.Label>
        <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
          {description}
        </Text>
        <Flex align='center' gap={1}>
          <Input
            {...numberInputProps}
            value={value}
            onChange={e => onChange(e.target.value)}
            placeholder={placeholder}
            min={min}
            max={max}
          />
          <Text fontSize='xs' color='whiteAlpha.600'>
            {unit}
          </Text>
        </Flex>
        <Field.ErrorText fontSize='xs'>{error}</Field.ErrorText>
      </Flex>
    </Field.Root>
  )
}

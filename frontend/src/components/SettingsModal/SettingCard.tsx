import type { ComponentType, ReactNode } from 'react'

import { Box, Field, Flex, Text } from '@chakra-ui/react'

interface SettingCardProps {
  title: string
  description: ReactNode
  icon?: ComponentType<{ className?: string }>
  iconColor?: string
  statusColor?: string
  statusTitle?: string
  opacity?: number
  action?: ReactNode
  note?: string
  children: ReactNode
}

export function SettingCard({
  title,
  description,
  icon: Icon,
  iconColor,
  statusColor,
  statusTitle,
  opacity,
  action,
  note,
  children,
}: SettingCardProps) {
  return (
    <Box
      role='group'
      aria-label={title}
      layerStyle='card'
      p={2}
      display='flex'
      flexDirection='column'
      height='100%'
      opacity={opacity}
    >
      <Flex
        align='center'
        justify={action ? 'space-between' : 'flex-start'}
        gap={1.5}
        mb={1}
      >
        <Flex align='center' gap={1.5}>
          {Icon && (
            <Box as={Icon} width='10px' height='10px' color={iconColor} />
          )}
          <Text fontSize='sm' fontWeight='500' color='fg'>
            {title}
          </Text>
          {statusColor && (
            <Box
              width='5px'
              height='5px'
              borderRadius='full'
              bg={statusColor}
              title={statusTitle}
            />
          )}
        </Flex>
        {action}
      </Flex>
      <Text fontSize='xs' color='fg.subtle' lineHeight='1.3' flex='1'>
        {description}
      </Text>
      {note && (
        <Text fontSize='xs' color='danger.fg' lineHeight='1.3' mt={1}>
          {note}
        </Text>
      )}
      <Box borderTop='1px solid' borderColor='border.subtle' mt={3} pt={3}>
        {children}
      </Box>
    </Box>
  )
}

export function SettingRow({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <Field.Root
      orientation='horizontal'
      justifyContent='flex-end'
      alignItems='center'
      gap={2}
    >
      <Field.Label
        flex='none'
        fontSize='xs'
        fontWeight='normal'
        color='fg.subtle'
        m={0}
      >
        {label}
      </Field.Label>
      {children}
    </Field.Root>
  )
}

export const compactInputProps = {
  layerStyle: 'field' as const,
  size: 'xs' as const,
  height: '22px',
  bg: 'bg.canvas',
  textAlign: 'center' as const,
  _placeholder: { color: 'fg.subtle' },
}

export const compactActionButtonProps = {
  size: '2xs' as const,
  variant: 'outline' as const,
  height: '18px',
  fontSize: '10px',
  color: 'fg.subtle',
  borderColor: 'border.emphasized',
  _hover: { borderColor: 'border.strong', bg: 'bg.faint' },
  px: 1.5,
}

export const isDigitsUpTo = (value: string, max: number) =>
  value === '' || (/^\d+$/.test(value) && parseInt(value, 10) <= max)

export const LOAD_FAILED_NOTE =
  'Failed to load current values. This section will not be saved.'

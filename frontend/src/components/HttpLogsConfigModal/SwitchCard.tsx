import { Box, Flex, Text } from '@chakra-ui/react'

import { Switch } from '@/components/ui/switch'

import { cardProps } from './cardProps'

interface SwitchCardProps {
  title: string
  description: string
  checked: boolean
  onCheckedChange: (checked: boolean) => void
}

export function SwitchCard({
  title,
  description,
  checked,
  onCheckedChange,
}: SwitchCardProps) {
  return (
    <Box {...cardProps}>
      <Flex direction='column' gap={2}>
        <Text fontSize='sm' fontWeight='500' color='white'>
          {title}
        </Text>
        <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
          {description}
        </Text>
        <Box alignSelf='flex-start'>
          <Switch
            aria-label={title}
            checked={checked}
            onCheckedChange={details => onCheckedChange(details.checked)}
            colorPalette='blue'
          />
        </Box>
      </Flex>
    </Box>
  )
}

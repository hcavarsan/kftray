import { Box, Flex, Input, Slider, Stack, Text } from '@chakra-ui/react'

interface PollingIntervalFieldProps {
  value: number
  onChange: (value: number) => void
}

export function PollingIntervalField({
  value,
  onChange,
}: PollingIntervalFieldProps) {
  return (
    <Stack gap={2} mt={2}>
      <Flex justify='space-between' align='center'>
        <Text fontSize='xs' color='fg.muted'>
          Polling Interval (minutes)
        </Text>
        <Input
          value={value === 0 ? 'off' : `${value} min`}
          readOnly
          width='65px'
          height='24px'
          textAlign='center'
          bg='bg.surface'
          borderColor='border'
          fontSize='11px'
          _disabled={{
            opacity: 0.8,
            cursor: 'default',
          }}
        />
      </Flex>
      <Box>
        <Slider.Root
          value={[value]}
          min={0}
          max={120}
          step={5}
          onValueChange={details => onChange(details.value[0])}
        >
          <Slider.Control>
            <Slider.Track>
              <Slider.Range />
            </Slider.Track>
            <Slider.Thumb index={0} />
          </Slider.Control>
        </Slider.Root>
      </Box>
      <Flex justify='space-between' align='center'>
        <Text fontSize='xs' color='fg.muted'>
          Disabled
        </Text>
        <Text fontSize='xs' color='fg.muted'>
          120 min
        </Text>
      </Flex>
    </Stack>
  )
}

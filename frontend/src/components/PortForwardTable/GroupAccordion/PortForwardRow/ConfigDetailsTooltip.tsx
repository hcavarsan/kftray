import { Box, Text } from '@chakra-ui/react'

import type {
  ConfigDetail,
  StatusInfo,
} from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/statusInfo'

interface ConfigDetailsTooltipProps {
  status: StatusInfo
  details: ConfigDetail[]
}

export function ConfigDetailsTooltip({
  status,
  details,
}: ConfigDetailsTooltipProps) {
  return (
    <Box p={1}>
      <Text fontSize='xs' fontWeight='medium'>
        Status: {status.status}
      </Text>
      <Box borderTop='1px solid' borderColor='border.emphasized' pt={1} mt={1}>
        {details.map(detail => (
          <Text fontSize='xs' key={detail.label}>
            <strong>{detail.label}:</strong> {detail.value}
          </Text>
        ))}
      </Box>
    </Box>
  )
}

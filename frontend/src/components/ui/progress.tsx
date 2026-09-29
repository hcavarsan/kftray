import type { RefAttributes } from 'react'

import { Progress as ChakraProgress } from '@chakra-ui/react'

export function ProgressBar({
  ref,
  ...props
}: ChakraProgress.TrackProps & RefAttributes<HTMLDivElement>) {
  return (
    <ChakraProgress.Track {...props} ref={ref}>
      <ChakraProgress.Range />
    </ChakraProgress.Track>
  )
}

export const ProgressRoot = ChakraProgress.Root

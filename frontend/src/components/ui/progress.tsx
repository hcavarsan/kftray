import { forwardRef } from 'react'

import { Progress as ChakraProgress } from '@chakra-ui/react'

export const ProgressBar = forwardRef<
  HTMLDivElement,
  ChakraProgress.TrackProps
>((props, ref) => {
  return (
    <ChakraProgress.Track {...props} ref={ref}>
      <ChakraProgress.Range />
    </ChakraProgress.Track>
  )
})

export const ProgressRoot = ChakraProgress.Root

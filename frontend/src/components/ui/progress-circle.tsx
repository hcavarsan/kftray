import { forwardRef } from 'react'

import type { SystemStyleObject } from '@chakra-ui/react'
import {
  AbsoluteCenter,
  ProgressCircle as ChakraProgressCircle,
} from '@chakra-ui/react'

export const ProgressCircleRoot = ChakraProgressCircle.Root

interface ProgressCircleRingProps extends ChakraProgressCircle.CircleProps {
  trackColor?: SystemStyleObject['stroke']
  cap?: SystemStyleObject['strokeLinecap']
}

export const ProgressCircleRing = forwardRef<
  SVGSVGElement,
  ProgressCircleRingProps
>((props, ref) => {
  const { trackColor, cap, color, ...rest } = props

  return (
    <ChakraProgressCircle.Circle {...rest} ref={ref}>
      <ChakraProgressCircle.Track stroke={trackColor} />
      <ChakraProgressCircle.Range stroke={color} strokeLinecap={cap} />
    </ChakraProgressCircle.Circle>
  )
})

export const ProgressCircleValueText = forwardRef<
  HTMLDivElement,
  ChakraProgressCircle.ValueTextProps
>((props, ref) => {
  return (
    <AbsoluteCenter>
      <ChakraProgressCircle.ValueText {...props} ref={ref} />
    </AbsoluteCenter>
  )
})

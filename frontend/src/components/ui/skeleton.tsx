import { forwardRef } from 'react'

import type {
  CircleProps,
  SkeletonProps as ChakraSkeletonProps,
} from '@chakra-ui/react'
import { Circle, Skeleton as ChakraSkeleton, Stack } from '@chakra-ui/react'

export interface SkeletonCircleProps extends ChakraSkeletonProps {
  size?: CircleProps['size']
}

export const SkeletonCircle = (props: SkeletonCircleProps) => {
  const { size, ...rest } = props

  return (
    <Circle size={size} asChild>
      <ChakraSkeleton {...rest} />
    </Circle>
  )
}

export interface SkeletonTextProps extends ChakraSkeletonProps {
  noOfLines?: number
}

export const SkeletonText = forwardRef<HTMLDivElement, SkeletonTextProps>(
  (props, ref) => {
    const { noOfLines = 3, gap, ...rest } = props

    return (
      <Stack gap={gap} width='full' ref={ref}>
        {Array.from({ length: noOfLines }).map((_, index) => (
          <ChakraSkeleton
            height='4'
            key={index}
            {...props}
            _last={{ maxW: '80%' }}
            {...rest}
          />
        ))}
      </Stack>
    )
  },
)

export const Skeleton = ChakraSkeleton

import { forwardRef } from 'react'
import { HiOutlineInformationCircle } from 'react-icons/hi'

import { IconButton, Progress as ChakraProgress } from '@chakra-ui/react'

import { ToggleTip } from './toggle-tip'

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
export const ProgressValueText = ChakraProgress.ValueText

export interface ProgressLabelProps extends ChakraProgress.LabelProps {
  info?: React.ReactNode
}

export const ProgressLabel = forwardRef<HTMLDivElement, ProgressLabelProps>(
  (props, ref) => {
    const { children, info, ...rest } = props

    return (
      <ChakraProgress.Label {...rest} ref={ref}>
        {children}
        {info && (
          <ToggleTip content={info}>
            <IconButton variant='ghost' aria-label='info' size='2xs' ms='1'>
              <HiOutlineInformationCircle />
            </IconButton>
          </ToggleTip>
        )}
      </ChakraProgress.Label>
    )
  },
)

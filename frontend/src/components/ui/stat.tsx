import { forwardRef } from 'react'
import { HiOutlineInformationCircle } from 'react-icons/hi'

import {
  Badge,
  type BadgeProps,
  FormatNumber,
  IconButton,
  Stat as ChakraStat,
} from '@chakra-ui/react'

import { ToggleTip } from './toggle-tip'

interface StatLabelProps extends ChakraStat.LabelProps {
  info?: React.ReactNode
}

export const StatLabel = forwardRef<HTMLDivElement, StatLabelProps>(
  (props, ref) => {
    const { info, children, ...rest } = props

    return (
      <ChakraStat.Label {...rest} ref={ref}>
        {children}
        {info && (
          <ToggleTip content={info}>
            <IconButton variant='ghost' aria-label='info' size='2xs'>
              <HiOutlineInformationCircle />
            </IconButton>
          </ToggleTip>
        )}
      </ChakraStat.Label>
    )
  },
)

interface StatValueTextProps extends ChakraStat.ValueTextProps {
  value?: number
  formatOptions?: Intl.NumberFormatOptions
}

export const StatValueText = forwardRef<HTMLDivElement, StatValueTextProps>(
  (props, ref) => {
    const { value, formatOptions, children, ...rest } = props

    return (
      <ChakraStat.ValueText {...rest} ref={ref}>
        {children ||
          (value != null && <FormatNumber value={value} {...formatOptions} />)}
      </ChakraStat.ValueText>
    )
  },
)

export const StatUpTrend = forwardRef<HTMLDivElement, BadgeProps>(
  (props, ref) => {
    return (
      <Badge colorPalette='green' gap='0' {...props} ref={ref}>
        <ChakraStat.UpIndicator />
        {props.children}
      </Badge>
    )
  },
)

export const StatDownTrend = forwardRef<HTMLDivElement, BadgeProps>(
  (props, ref) => {
    return (
      <Badge colorPalette='red' gap='0' {...props} ref={ref}>
        <ChakraStat.DownIndicator />
        {props.children}
      </Badge>
    )
  },
)

export const StatRoot = ChakraStat.Root
export const StatHelpText = ChakraStat.HelpText
export const StatValueUnit = ChakraStat.ValueUnit

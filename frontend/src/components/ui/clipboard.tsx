import { forwardRef } from 'react'
import { LuCheck, LuClipboard, LuLink } from 'react-icons/lu'

import type { ButtonProps, InputProps } from '@chakra-ui/react'
import {
  Button,
  Clipboard as ChakraClipboard,
  IconButton,
  Input,
} from '@chakra-ui/react'

const ClipboardIcon = forwardRef<
  HTMLDivElement,
  ChakraClipboard.IndicatorProps
>((props, ref) => {
  return (
    <ChakraClipboard.Indicator copied={<LuCheck />} {...props} ref={ref}>
      <LuClipboard />
    </ChakraClipboard.Indicator>
  )
})

const ClipboardCopyText = forwardRef<
  HTMLDivElement,
  ChakraClipboard.IndicatorProps
>((props, ref) => {
  return (
    <ChakraClipboard.Indicator copied='Copied' {...props} ref={ref}>
      Copy
    </ChakraClipboard.Indicator>
  )
})

export const ClipboardLabel = forwardRef<
  HTMLLabelElement,
  ChakraClipboard.LabelProps
>((props, ref) => {
  return (
    <ChakraClipboard.Label
      textStyle='sm'
      fontWeight='medium'
      display='inline-block'
      mb='1'
      {...props}
      ref={ref}
    />
  )
})

export const ClipboardButton = forwardRef<HTMLButtonElement, ButtonProps>(
  (props, ref) => {
    return (
      <ChakraClipboard.Trigger asChild>
        <Button ref={ref} size='sm' variant='surface' {...props}>
          <ClipboardIcon />
          <ClipboardCopyText />
        </Button>
      </ChakraClipboard.Trigger>
    )
  },
)

export const ClipboardLink = forwardRef<HTMLButtonElement, ButtonProps>(
  (props, ref) => {
    return (
      <ChakraClipboard.Trigger asChild>
        <Button
          unstyled
          variant='plain'
          size='xs'
          display='inline-flex'
          alignItems='center'
          gap='2'
          ref={ref}
          {...props}
        >
          <LuLink />
          <ClipboardCopyText />
        </Button>
      </ChakraClipboard.Trigger>
    )
  },
)

export const ClipboardIconButton = forwardRef<HTMLButtonElement, ButtonProps>(
  (props, ref) => {
    return (
      <ChakraClipboard.Trigger asChild>
        <IconButton ref={ref} size='xs' variant='subtle' {...props}>
          <ClipboardIcon />
          <ClipboardCopyText srOnly />
        </IconButton>
      </ChakraClipboard.Trigger>
    )
  },
)

export const ClipboardInput = forwardRef<HTMLInputElement, InputProps>(
  (props, ref) => {
    return (
      <ChakraClipboard.Input asChild>
        <Input ref={ref} {...props} />
      </ChakraClipboard.Input>
    )
  },
)

export const ClipboardRoot = ChakraClipboard.Root

import type { ComponentProps, ReactNode } from 'react'

import { Box, IconButton } from '@chakra-ui/react'

import { Tooltip } from '@/components/ui/tooltip'
import type { ConfigView, Facet } from '@/types'

export interface ViewMenuProps {
  view: ConfigView
  facets: Facet[]
  setView: (view: ConfigView) => void
}

interface ToolbarIconButtonProps extends ComponentProps<typeof IconButton> {
  active?: boolean
  badge?: number
}

export const ToolbarIconButton = ({
  active = false,
  badge = 0,
  children,
  ...rest
}: ToolbarIconButtonProps) => (
  <IconButton
    size='xs'
    variant='ghost'
    h='26px'
    w='26px'
    minW='26px'
    position='relative'
    overflow='visible'
    borderRadius='md'
    border='1px solid'
    borderColor={active ? 'accent.emphasized' : 'border'}
    bg={active ? 'accent.subtle' : 'bg.faint'}
    color={active ? 'accent.fg' : 'fg.muted'}
    _hover={{
      bg: active ? 'accent.muted' : 'bg.hover',
      color: active ? 'accent.fg' : 'fg',
    }}
    _focusVisible={{
      outline: '1px solid',
      outlineColor: 'accent.focusRing',
      outlineOffset: '1px',
    }}
    {...rest}
  >
    {children}
    {badge > 0 && (
      <Box
        position='absolute'
        top='-6px'
        right='-6px'
        minW='12px'
        h='12px'
        px='3px'
        bg='accent.solid'
        color='fg'
        fontSize='8px'
        fontWeight='semibold'
        lineHeight='12px'
        textAlign='center'
        borderRadius='full'
        boxShadow='0 0 0 1.5px var(--chakra-colors-bg-surface)'
        pointerEvents='none'
      >
        {badge}
      </Box>
    )}
  </IconButton>
)

export const WithTooltip = ({
  content,
  children,
}: {
  content: string
  children: ReactNode
}) => (
  <Tooltip content={content}>
    <Box display='inline-flex'>{children}</Box>
  </Tooltip>
)

export const contentProps = {
  bg: 'bg.raised',
  border: '1px solid',
  borderColor: 'border',
  borderRadius: 'md',
  boxShadow: 'popover',
  minW: '200px',
  maxW: '260px',
  maxH: '300px',
  overflowY: 'auto' as const,
  py: 1,
}

export const itemProps = {
  fontSize: '11px',
  minH: '24px',
  py: 0,
  ps: 7,
  pe: 2.5,
  borderRadius: 'sm',
  mx: 1,
  color: 'fg',
  bg: 'transparent',
  _highlighted: { bg: 'bg.hover' },
}

export const sectionLabelProps = {
  fontSize: '10px',
  fontWeight: 'medium',
  color: 'fg.subtle',
  ps: 3.5,
  pe: 2.5,
  pt: 1.5,
  pb: 0.5,
}

export const separatorProps = {
  my: 1,
  borderColor: 'border.subtle',
}

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
    borderColor={active ? 'blue.500/35' : 'app.border'}
    bg={active ? 'blue.500/12' : 'whiteAlpha.50'}
    color={active ? 'blue.400' : 'whiteAlpha.700'}
    _hover={{
      bg: active ? 'blue.500/18' : 'whiteAlpha.100',
      color: active ? 'app.accentText' : 'whiteAlpha.900',
    }}
    _focusVisible={{
      outline: '1px solid',
      outlineColor: 'blue.500',
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
        bg='blue.500'
        color='white'
        fontSize='8px'
        fontWeight='semibold'
        lineHeight='12px'
        textAlign='center'
        borderRadius='full'
        boxShadow='0 0 0 1.5px var(--chakra-colors-app-panel)'
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
  bg: 'app.raised',
  border: '1px solid',
  borderColor: 'app.border',
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
  color: 'whiteAlpha.900',
  bg: 'transparent',
  _highlighted: { bg: 'whiteAlpha.100' },
}

export const sectionLabelProps = {
  fontSize: '10px',
  fontWeight: 'medium',
  color: 'whiteAlpha.500',
  ps: 3.5,
  pe: 2.5,
  pt: 1.5,
  pb: 0.5,
}

export const separatorProps = {
  my: 1,
  borderColor: 'app.subtle',
}

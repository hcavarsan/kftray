import React, { useMemo } from 'react'
import { ChevronDown, ChevronUp, RefreshCw, X } from 'lucide-react'

import { Box, Group } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { HeaderMenuProps } from '@/types'

const HeaderMenu: React.FC<HeaderMenuProps> = ({
  configs,
  selectedConfigs,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopSelectedPortForwarding,
  stopAllPortForwarding,
  isInitiating,
  isStopping,
  toggleExpandAll,
  expandedIndices,
  configsByContext,
  setSelectedConfigs,
}) => {
  const isSelectAllChecked = useMemo(() => {
    return configs.every(config =>
      selectedConfigs.some(selected => selected.id === config.id),
    )
  }, [configs, selectedConfigs])

  const handleCheckboxChange = ({
    checked,
  }: {
    checked: boolean | 'indeterminate'
  }) => {
    setSelectedConfigs(checked === true ? configs : [])
  }

  return (
    <Box
      display='flex'
      alignItems='center'
      justifyContent='space-between'
      width='100%'
      bg='#161616'
      px={3}
      py={3}
      borderTopRadius='none'
      borderTop='none'
      borderBottomRadius='lg'
      border='1px solid rgba(255, 255, 255, 0.08)'
      position='relative'
      zIndex={10}
      borderTopColor='rgba(255, 255, 255, 0.04)'
      mt='-1px'
    >
      <Group display='flex' alignItems='center' gap={3}>
        {/* Checkbox */}
        <Checkbox
          ml={2}
          size='sm'
          checked={isSelectAllChecked}
          onCheckedChange={handleCheckboxChange}
          css={{
            '& input': {
              width: '10px',
              height: '10px',
              background: '#1A1A1A',
              border: '1px solid rgba(255, 255, 255, 0.15)',
              borderRadius: '3px',
              '&:hover': {
                borderColor: 'rgba(255, 255, 255, 0.25)',
              },
            },
            '& input:checked': {
              background: '#3182CE',
              borderColor: '#3182CE',
            },
          }}
        />

        {/* Action Buttons */}
        <Group display='flex' alignItems='center' gap={2}>
          <Button
            size='xs'
            variant='ghost'
            disabled={
              isInitiating ||
              (selectedConfigs.length > 0
                ? selectedConfigs.every(selected => {
                    const currentConfig = configs.find(
                      c => c.id === selected.id,
                    )

                    return currentConfig && currentConfig.is_running
                  })
                : configs.every(config => config.is_running))
            }
            loading={isInitiating}
            loadingText='Starting...'
            onClick={
              selectedConfigs.length > 0
                ? startSelectedPortForwarding
                : () =>
                    initiatePortForwarding(
                      configs.filter(config => !config.is_running),
                    )
            }
            _hover={{ bg: 'whiteAlpha.100' }}
            height='26px'
            minWidth='90px'
            bg='whiteAlpha.50'
            px={2}
            borderRadius='md'
            border='1px solid rgba(255, 255, 255, 0.08)'
          >
            <Box as={RefreshCw} width='12px' height='12px' marginRight={1.5} />
            <span style={{ fontSize: '11px' }}>
              {selectedConfigs.length > 0 &&
              selectedConfigs.some(selected => {
                const currentConfig = configs.find(c => c.id === selected.id)

                return currentConfig && !currentConfig.is_running
              })
                ? 'Start Selected'
                : 'Start All'}
            </span>
          </Button>

          <Button
            size='xs'
            variant='ghost'
            disabled={
              isStopping ||
              (selectedConfigs.length > 0
                ? selectedConfigs.every(selected => {
                    const currentConfig = configs.find(
                      c => c.id === selected.id,
                    )

                    return currentConfig && !currentConfig.is_running
                  })
                : configs.every(config => !config.is_running))
            }
            loading={isStopping}
            loadingText='Stopping...'
            onClick={
              selectedConfigs.length > 0
                ? stopSelectedPortForwarding
                : stopAllPortForwarding
            }
            _hover={{ bg: 'whiteAlpha.100' }}
            height='26px'
            minWidth='90px'
            bg='whiteAlpha.50'
            px={2}
            borderRadius='md'
            border='1px solid rgba(255, 255, 255, 0.08)'
          >
            <Box as={X} width='12px' height='12px' marginRight={1.5} />
            <span style={{ fontSize: '11px' }}>
              {selectedConfigs.length > 0 &&
              selectedConfigs.some(selected => {
                const currentConfig = configs.find(c => c.id === selected.id)

                return currentConfig && currentConfig.is_running
              })
                ? 'Stop Selected'
                : 'Stop All'}
            </span>
          </Button>
        </Group>
      </Group>

      {/* Expand/Collapse Button */}
      <Button
        size='xs'
        variant='ghost'
        onClick={toggleExpandAll}
        _hover={{ bg: 'whiteAlpha.100' }}
        height='26px'
        minWidth='90px'
        bg='whiteAlpha.50'
        px={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
      >
        <span style={{ fontSize: '11px' }}>
          {expandedIndices.length === Object.keys(configsByContext).length
            ? 'Collapse All'
            : 'Expand All'}
        </span>
        <Box
          as={
            expandedIndices.length === Object.keys(configsByContext).length
              ? ChevronUp
              : ChevronDown
          }
          width='12px'
          height='12px'
          marginLeft={1.5}
        />
      </Button>
    </Box>
  )
}

export default HeaderMenu

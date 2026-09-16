import type React from 'react'
import { useMemo } from 'react'
import { ChevronDown, ChevronUp, Loader2, RefreshCw, X } from 'lucide-react'

import { Box, chakra, Group } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Tooltip } from '@/components/ui/tooltip'
import type { HeaderMenuProps } from '@/types'

const HeaderMenu: React.FC<HeaderMenuProps> = ({
  configs,
  selectedConfigs,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopSelectedPortForwarding,
  stopAllPortForwarding,
  abortStartOperation,
  abortStopOperation,
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

  const isStartBusy =
    isInitiating ||
    (selectedConfigs.length > 0
      ? selectedConfigs.every(selected => {
          const currentConfig = configs.find(c => c.id === selected.id)

          return currentConfig?.is_running
        })
      : configs.every(config => config.is_running))

  const isStopBusy =
    isStopping ||
    (selectedConfigs.length > 0
      ? selectedConfigs.every(selected => {
          const currentConfig = configs.find(c => c.id === selected.id)

          return currentConfig && !currentConfig.is_running
        })
      : configs.every(config => !config.is_running))

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
          <Tooltip
            content={
              isInitiating
                ? 'Starting port forwards...'
                : selectedConfigs.length > 0 &&
                    selectedConfigs.some(selected => {
                      const currentConfig = configs.find(
                        c => c.id === selected.id,
                      )

                      return currentConfig && !currentConfig.is_running
                    })
                  ? 'Start selected port forwards'
                  : 'Start all port forwards'
            }
            portalled={true}
            contentProps={{ zIndex: 100 }}
          >
            <Button
              size='xs'
              variant='ghost'
              disabled={isStartBusy}
              onClick={
                selectedConfigs.length > 0
                  ? () => void startSelectedPortForwarding()
                  : () =>
                      void initiatePortForwarding(
                        configs.filter(config => !config.is_running),
                      )
              }
              _hover={{ bg: isInitiating ? undefined : 'whiteAlpha.100' }}
              _disabled={{ cursor: 'not-allowed' }}
              height='26px'
              minWidth='90px'
              bg='whiteAlpha.50'
              px={2}
              borderRadius='md'
              border='1px solid rgba(255, 255, 255, 0.08)'
            >
              {isInitiating ? (
                <>
                  <Box
                    as={Loader2}
                    width='12px'
                    height='12px'
                    marginRight={1.5}
                    animation='spin 1s linear infinite'
                    css={{
                      '@keyframes spin': {
                        from: { transform: 'rotate(0deg)' },
                        to: { transform: 'rotate(360deg)' },
                      },
                    }}
                  />
                  <span style={{ fontSize: '11px' }}>Starting...</span>
                </>
              ) : (
                <>
                  <Box
                    as={RefreshCw}
                    width='12px'
                    height='12px'
                    marginRight={1.5}
                  />
                  <span style={{ fontSize: '11px' }}>
                    {selectedConfigs.length > 0 &&
                    selectedConfigs.some(selected => {
                      const currentConfig = configs.find(
                        c => c.id === selected.id,
                      )

                      return currentConfig && !currentConfig.is_running
                    })
                      ? 'Start Selected'
                      : 'Start All'}
                  </span>
                </>
              )}
            </Button>
          </Tooltip>

          {isInitiating && (
            <Tooltip content='Cancel' portalled contentProps={{ zIndex: 101 }}>
              <chakra.button
                type='button'
                aria-label='Cancel'
                display='inline-flex'
                alignItems='center'
                justifyContent='center'
                padding='6px'
                borderRadius='sm'
                cursor='pointer'
                bg='transparent'
                border='none'
                _hover={{ bg: 'red.700' }}
                onClick={() => abortStartOperation()}
              >
                <Box as={X} width='10px' height='10px' color='red.300' />
              </chakra.button>
            </Tooltip>
          )}

          <Tooltip
            content={
              isStopping
                ? 'Stopping port forwards...'
                : selectedConfigs.length > 0 &&
                    selectedConfigs.some(selected => {
                      const currentConfig = configs.find(
                        c => c.id === selected.id,
                      )

                      return currentConfig?.is_running
                    })
                  ? 'Stop selected port forwards'
                  : 'Stop all port forwards'
            }
            portalled={true}
            contentProps={{ zIndex: 100 }}
          >
            <Button
              size='xs'
              variant='ghost'
              disabled={isStopBusy}
              onClick={
                selectedConfigs.length > 0
                  ? () => void stopSelectedPortForwarding()
                  : () => void stopAllPortForwarding()
              }
              _hover={{ bg: isStopping ? undefined : 'whiteAlpha.100' }}
              _disabled={{ cursor: 'not-allowed' }}
              height='26px'
              minWidth='90px'
              bg='whiteAlpha.50'
              px={2}
              borderRadius='md'
              border='1px solid rgba(255, 255, 255, 0.08)'
            >
              {isStopping ? (
                <>
                  <Box
                    as={Loader2}
                    width='12px'
                    height='12px'
                    marginRight={1.5}
                    animation='spin 1s linear infinite'
                    css={{
                      '@keyframes spin': {
                        from: { transform: 'rotate(0deg)' },
                        to: { transform: 'rotate(360deg)' },
                      },
                    }}
                  />
                  <span style={{ fontSize: '11px' }}>Stopping...</span>
                </>
              ) : (
                <>
                  <Box as={X} width='12px' height='12px' marginRight={1.5} />
                  <span style={{ fontSize: '11px' }}>
                    {selectedConfigs.length > 0 &&
                    selectedConfigs.some(selected => {
                      const currentConfig = configs.find(
                        c => c.id === selected.id,
                      )

                      return currentConfig?.is_running
                    })
                      ? 'Stop Selected'
                      : 'Stop All'}
                  </span>
                </>
              )}
            </Button>
          </Tooltip>

          {isStopping && (
            <Tooltip content='Cancel' portalled contentProps={{ zIndex: 101 }}>
              <chakra.button
                type='button'
                aria-label='Cancel'
                display='inline-flex'
                alignItems='center'
                justifyContent='center'
                padding='6px'
                borderRadius='sm'
                cursor='pointer'
                bg='transparent'
                border='none'
                _hover={{ bg: 'red.700' }}
                onClick={() => abortStopOperation()}
              >
                <Box as={X} width='10px' height='10px' color='red.300' />
              </chakra.button>
            </Tooltip>
          )}
        </Group>
      </Group>

      {/* Expand/Collapse Button */}
      <Tooltip
        content={
          expandedIndices.length === Object.keys(configsByContext).length
            ? 'Collapse all contexts'
            : 'Expand all contexts'
        }
        portalled={true}
        contentProps={{ zIndex: 100 }}
      >
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
      </Tooltip>
    </Box>
  )
}

export default HeaderMenu

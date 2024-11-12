import React from 'react'
import { ChevronDown, ChevronUp, RefreshCw, X } from 'lucide-react'

import { Box, Group } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { HeaderMenuProps } from '@/types'

// Add this type
type CheckedState = boolean | 'indeterminate'

const HeaderMenu: React.FC<HeaderMenuProps> = ({
  isSelectAllChecked,
  setIsSelectAllChecked,
  configs,
  selectedConfigs,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopAllPortForwarding,
  isInitiating,
  isStopping,
  toggleExpandAll,
  expandedIndices,
  configsByContext,
  setSelectedConfigs,
}) => {
  const handleCheckboxChange = ({ checked }: { checked: CheckedState }) => {
    // Convert the checked state to boolean
    const isChecked = checked === true

    setIsSelectAllChecked(isChecked)
    setSelectedConfigs(isChecked ? configs : [])
  }

  return (
    <Box
      display="flex"
      alignItems="center"
      justifyContent="space-between"
      width="100%"
      bg="#161616"
      px={3}
      py={2}
      borderRadius="md"
      border="1px solid rgba(255, 255, 255, 0.08)"
    >
      <Group
        display="flex"
        alignItems="center"
        gap={2}
      >
        {/* Checkbox */}
        <Box
          display="flex"
          alignItems="center"
          px={1}
        >
          <Checkbox
            checked={isSelectAllChecked}
            onCheckedChange={handleCheckboxChange}
            css={{
              '& input': {
                width: '12px',
                height: '12px',
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
        </Box>

        {/* Start Button */}
        <Button
          size="2xs"
          variant="ghost"
          disabled={
            isInitiating ||
            (!selectedConfigs.length && !configs.some(config => !config.is_running))
          }
          loading={isInitiating}
          loadingText="Starting..."
          onClick={
            selectedConfigs.length > 0
              ? startSelectedPortForwarding
              : () => initiatePortForwarding(configs.filter(config => !config.is_running))
          }
          _hover={{ bg: 'whiteAlpha.100' }}
          height="24px"
          minWidth="90px"
          bg="whiteAlpha.50"
          px={2}
        >
          <Box as={RefreshCw} width="12px" height="12px" marginRight={1.5} />
          <span style={{ fontSize: '11px' }}>
            {selectedConfigs.length > 0 ? 'Start Selected' : 'Start All'}
          </span>
        </Button>

        {/* Stop Button */}
        <Button
          size="2xs"
          variant="ghost"
          disabled={isStopping || !configs.some(config => config.is_running)}
          loading={isStopping}
          loadingText="Stopping..."
          onClick={stopAllPortForwarding}
          _hover={{ bg: 'whiteAlpha.100' }}
          height="24px"
          minWidth="70px"
          bg="whiteAlpha.50"
          px={2}
        >
          <Box as={X} width="12px" height="12px" marginRight={1.5} />
          <span style={{ fontSize: '11px' }}>Stop All</span>
        </Button>
      </Group>

      {/* Expand/Collapse Button */}
      <Button
        size="2xs"
        variant="ghost"
        onClick={toggleExpandAll}
        _hover={{ bg: 'whiteAlpha.100' }}
        height="24px"
        minWidth="90px"
        bg="whiteAlpha.50"
        px={2}
      >
        <span style={{ fontSize: '11px' }}>
          {expandedIndices.length === Object.keys(configsByContext).length
            ? 'Collapse All'
            : 'Expand All'}
        </span>
        <Box
          as={expandedIndices.length === Object.keys(configsByContext).length
            ? ChevronUp
            : ChevronDown}
          width="12px"
          height="12px"
          marginLeft={1.5}
        />
      </Button>
    </Box>
  )
}

export default HeaderMenu

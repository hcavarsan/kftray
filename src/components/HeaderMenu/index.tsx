import React from 'react'
import { MdClose, MdRefresh } from 'react-icons/md'

import { ChevronDownIcon, ChevronUpIcon } from '@chakra-ui/icons'
import { Button, ButtonGroup, Checkbox } from '@chakra-ui/react'

import { HeaderMenuProps } from '../../types'

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
  return (
    <>
      <ButtonGroup variant='outline'>
        <Checkbox
          isChecked={isSelectAllChecked}
          onChange={e => {
            setIsSelectAllChecked(e.target.checked)
            if (e.target.checked) {
              setSelectedConfigs(configs)
            } else {
              setSelectedConfigs([])
            }
          }}
          mr={2}
          ml={2}
          size='sm'
        />
        <Button
          leftIcon={<MdRefresh />}
          colorScheme='facebook'
          isLoading={isInitiating}
          loadingText={isInitiating ? 'Starting...' : null}
          onClick={
            selectedConfigs.length > 0
              ? startSelectedPortForwarding
              : () =>
                initiatePortForwarding(
                  configs.filter(config => !config.isRunning),
                )
          }
          isDisabled={
            isInitiating ||
            (!selectedConfigs.length &&
              !configs.some(config => !config.isRunning))
          }
          size='xs'
        >
          {selectedConfigs.length > 0 ? 'Start Selected' : 'Start All'}
        </Button>
        <Button
          leftIcon={<MdClose />}
          colorScheme='facebook'
          isLoading={isStopping}
          loadingText='Stopping...'
          onClick={stopAllPortForwarding}
          isDisabled={isStopping || !configs.some(config => config.isRunning)}
          size='xs'
        >
          Stop All
        </Button>
      </ButtonGroup>

      <Button
        onClick={toggleExpandAll}
        size='xs'
        colorScheme='facebook'
        variant='outline'
        rightIcon={
          expandedIndices.length === Object.keys(configsByContext).length ? (
            <ChevronUpIcon />
          ) : (
            <ChevronDownIcon />
          )
        }
      >
        {expandedIndices.length === Object.keys(configsByContext).length
          ? 'Collapse All'
          : 'Expand All'}
      </Button>
    </>
  )
}

export default HeaderMenu

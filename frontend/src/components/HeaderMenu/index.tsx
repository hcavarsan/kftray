import React from 'react'
import { MdClose, MdRefresh } from 'react-icons/md'

import { ChevronDownIcon, ChevronUpIcon } from '@chakra-ui/icons'
import { Button, ButtonGroup, Checkbox } from '@chakra-ui/react'

import useConfigStore from '../../store'
import { HeaderMenuProps } from '../../types'

const HeaderMenu: React.FC<HeaderMenuProps> = ({
  isSelectAllChecked,
  setIsSelectAllChecked,
  selectedConfigs,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopAllPortForwarding,
  toggleExpandAll,
  expandedIndices,
  configsByContext,
  setSelectedConfigs,
}) => {
  const { configs, isInitiating, isStopping, configState } = useConfigStore(
    state => ({
      configs: state.configs,
      isInitiating: state.isInitiating,
      isStopping: state.isStopping,
      configState: state.configState,
    }),
  )

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
                  configs.filter(config => !configState[config.id]?.running),
                )
          }
          isDisabled={
            isInitiating ||
            (!selectedConfigs.length &&
              !configs.some(config => !configState[config.id]?.running))
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
          isDisabled={
            isStopping ||
            !Object.values(configState).some(config => config.running)
          }
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

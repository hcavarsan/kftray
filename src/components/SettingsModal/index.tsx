import React, { useState } from 'react'
import ReactSelect from 'react-select'

import {
  Box,
  Button,
  Center,
  FormControl,
  FormLabel,
  Input,
  Modal,
  ModalBody,
  ModalCloseButton,
  ModalContent,
  ModalFooter,
  ModalOverlay,
} from '@chakra-ui/react'

import theme from '../../assets/theme'
import { SettingsModalProps } from '../../types'

const SettingsModal: React.FC<SettingsModalProps> = ({
  isSettingsModalOpen,
  closeModal,
}) => {
  const [settingInputValue, setSettingInputValue] = useState('')

  const handleInputChange = (e: {
    target: { value: React.SetStateAction<string> }
  }) => setSettingInputValue(e.target.value)

  const handleSaveSettings = (e: { preventDefault: () => void }) => {
    e.preventDefault()
    console.log('Saving setting:', settingInputValue)
    closeModal()
    setSettingInputValue('')
  }

  const handleCancel = () => {
    closeModal()
    setSettingInputValue('')
  }

  return (
    <Center>
      <Modal isOpen={isSettingsModalOpen} onClose={handleCancel} size='xl'>
        <ModalOverlay bg='transparent' />
        <ModalContent mx={5} my={5} mt={8}>
          <ModalCloseButton />
          <ModalBody p={2} mt={3}>
            <form onSubmit={handleSaveSettings}>
              <FormControl
                display='flex'
                alignItems='center'
                flexWrap='wrap'
                p={2}
              >
                <Box width={{ base: '100%', sm: '100%' }} pl={2}>
                  <FormLabel htmlFor='settingInput'>Setting Name</FormLabel>
                  <Input
                    id='settingInput'
                    type='text'
                    value={settingInputValue}
                    onChange={handleInputChange}
                    placeholder='Enter your setting name here' // Placeholder text
                    name='settingInput'
                    size='sm'
                    height='36px'
                    bg={theme.colors.gray[800]}
                    borderColor={theme.colors.gray[700]}
                    _hover={{ borderColor: theme.colors.gray[600] }}
                    _placeholder={{ color: theme.colors.gray[500] }}
                    color={theme.colors.gray[300]}
                  />
                </Box>
              </FormControl>

              <ModalFooter justifyContent='flex-end' p={2} mt={5}>
                <Button variant='outline' onClick={handleCancel} size='xs'>
                  Cancel
                </Button>
                <Button type='submit' colorScheme='blue' size='xs' ml={3}>
                  Save Settings
                </Button>
              </ModalFooter>
            </form>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default SettingsModal

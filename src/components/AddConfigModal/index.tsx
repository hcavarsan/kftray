import React from 'react'

import {
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

import { ConfigProps } from '../../types'

const AddConfigModal: React.FC<ConfigProps> = ({
  isModalOpen,
  closeModal,
  newConfig,
  handleInputChange,
  handleSaveConfig,
  isEdit,
}) => {
  const onSubmit = isEdit
    ? handleSaveConfig
    : (event: React.FormEvent<Element>) => {
      event.preventDefault()
      handleSaveConfig(event)
    }

  return (
    <Center>
      <Modal isOpen={isModalOpen} onClose={closeModal} size='sm'>
        <ModalOverlay bg='transparent' />
        <ModalContent mt='40px' w='fit-content'>
          <ModalCloseButton />
          <ModalBody pb={2}>
            <FormControl as='form' onSubmit={onSubmit}>
              <FormLabel htmlFor='context'>Context</FormLabel>
              <Input
                id='context'
                value={newConfig.context || ''}
                name='context'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel htmlFor='namespace'>Namespace</FormLabel>
              <Input
                id='namespace'
                value={newConfig.namespace || ''}
                name='namespace'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel htmlFor='service'>Service</FormLabel>
              <Input
                id='service'
                value={newConfig.service || ''}
                name='service'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel htmlFor='local_port'>Local Port</FormLabel>
              <Input
                id='local_port'
                type='number'
                value={newConfig.local_port || ''}
                name='local_port'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel htmlFor='remote_port'>Remote Port</FormLabel>
              <Input
                id='remote_port'
                type='number'
                value={newConfig.remote_port || ''}
                name='remote_port'
                onChange={handleInputChange}
                size='sm'
              />
              <ModalFooter>
                <Button variant='outline' size='sm' mr={3} onClick={closeModal}>
                  Cancel
                </Button>
                <Button type='submit' colorScheme='blue' size='sm'>
                  {isEdit ? 'Save Changes' : 'Add Config'}
                </Button>
              </ModalFooter>
            </FormControl>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default AddConfigModal

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
} from "@chakra-ui/react"

interface AddConfigModalProps {
  isModalOpen: boolean
  closeModal: () => void
  newConfig: {
    id: number
    service: string
    context: string
    local_port: number
    remote_port: number
    namespace: string
  }
  handleInputChange: (event: React.ChangeEvent<HTMLInputElement>) => void
  handleSaveConfig: (e: React.FormEvent) => Promise<void>
  handleEditSubmit: (e: React.FormEvent) => Promise<void>
  cancelRef: React.RefObject<HTMLElement>
  isEdit: boolean
}

const AddConfigModal: React.FC<AddConfigModalProps> = props => {
  const {
    isModalOpen,
    closeModal,
    newConfig,
    handleInputChange,
    handleSaveConfig,
    isEdit,
  } = props

  return (
    <Center>
      <Modal isOpen={isModalOpen} onClose={closeModal} size='sm'>
        <ModalContent mt='40px' width='fit-content'>
          {" "}
          {/* Adjusts the top margin and centers horizontally */}
          <ModalCloseButton />
          <ModalBody mt='10px ' width='fit-content' pb={2}>
            <FormControl>
              <FormLabel>Context</FormLabel>
              <Input
                value={newConfig.context}
                name='context'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel>Namespace</FormLabel>
              <Input
                value={newConfig.namespace}
                name='namespace'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel>Service</FormLabel>
              <Input
                value={newConfig.service}
                name='service'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel>Local Port</FormLabel>
              <Input
                value={newConfig.local_port}
                name='local_port'
                onChange={handleInputChange}
                size='sm'
              />
              <FormLabel>Remote Port</FormLabel>
              <Input
                value={newConfig.remote_port}
                name='remote_port'
                onChange={handleInputChange}
                size='sm'
              />
            </FormControl>
          </ModalBody>
          <ModalFooter>
            <Button
              colorScheme='ghost'
              variant='outline'
              size='sm'
              mr={6}
              onClick={closeModal}
            >
              Close
            </Button>
            {isEdit ? (
              <Button
                colorScheme='blue'
                size='sm'
                mr={1}
                onClick={handleSaveConfig}
              >
                Save Changes
              </Button>
            ) : (
              <Button
                colorScheme='facebook'
                size='sm'
                mr={1}
                onClick={handleSaveConfig}
              >
                Add Config
              </Button>
            )}
          </ModalFooter>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export { AddConfigModal }

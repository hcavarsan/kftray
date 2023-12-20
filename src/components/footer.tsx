import { MdAdd, MdFileDownload, MdFileUpload } from 'react-icons/md'

import { Button, Flex } from '@chakra-ui/react'

interface FooterProps {
  openModal: () => void
  handleExportConfigs: () => void
  handleImportConfigs: () => void
}

const Footer: React.FC<FooterProps> = props => {
  const { openModal, handleExportConfigs, handleImportConfigs } = props


  
  return (
    <Flex direction='column' align='center' mt='30px' width='100%' mb='30px'>
      <Button
        leftIcon={<MdAdd />}
        variant='solid'
        size='xs'
        colorScheme='facebook'
        onClick={openModal}
        width='80%' // Set a consistent width for the button
      >
        Add New Config
      </Button>
      <Flex direction='row' justify='space-between' mt={2} width='80%'>
        <Button
          onClick={handleExportConfigs}
          leftIcon={<MdFileUpload />}
          size='xs'
          variant='solid'
          width='48%' // Setting width to less than half allows for space between buttons
          colorScheme='facebook'
        >
          Export Configs
        </Button>

        <Button
          onClick={handleImportConfigs}
          leftIcon={<MdFileDownload />}
          size='xs'
          variant='solid'
          width='48%' // Same width as the previous button
          colorScheme='facebook'
        >
          Import Configs
        </Button>
      </Flex>
    </Flex>
  )
}

export { Footer }

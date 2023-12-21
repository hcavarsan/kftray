import React from 'react'
import { MdAdd, MdFileDownload, MdFileUpload } from 'react-icons/md'

import { Button, Flex } from '@chakra-ui/react'

import { FooterProps } from '../../types'

const Footer: React.FC<FooterProps> = ({
  openModal,
  handleExportConfigs,
  handleImportConfigs,
}) => {
  return (
    <Flex direction='column' align='center' mt='30px' width='100%' mb='30px'>
      <Button
        leftIcon={<MdAdd />}
        colorScheme='facebook'
        onClick={openModal}
        width='80%'
        size='sm'
      >
        Add New Config
      </Button>
      <Flex direction='row' justify='space-between' mt={2} width='80%'>
        <Button
          leftIcon={<MdFileUpload />}
          colorScheme='facebook'
          onClick={handleExportConfigs}
          width='48%'
          size='sm'
        >
          Export Configs
        </Button>
        <Button
          leftIcon={<MdFileDownload />}
          colorScheme='facebook'
          onClick={handleImportConfigs}
          width='48%'
          size='sm'
        >
          Import Configs
        </Button>
      </Flex>
    </Flex>
  )
}

export default Footer

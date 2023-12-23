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
    <Flex
      as='footer'
      direction='column'
      align='center'
      width='100%'
      p='30px'
      position='absolute'
      left='0'
      bottom='1'
      boxShadow='0 -2px 10px 0 rgba(0,0,0,0.05)' // Optional: adds a shadow for better separation
      zIndex='sticky'
      mt={2}
      mb={5}
    >
      <Button
        leftIcon={<MdAdd />}
        colorScheme='facebook'
        onClick={openModal}
        width='80%'
        size='xs'
      >
        Add New Config
      </Button>
      <Flex direction='row' justify='space-between' mt={2} width='80%'>
        <Button
          leftIcon={<MdFileUpload />}
          colorScheme='facebook'
          onClick={handleExportConfigs}
          width='48%'
          size='xs'
        >
          Export Configs
        </Button>
        <Button
          leftIcon={<MdFileDownload />}
          colorScheme='facebook'
          onClick={handleImportConfigs}
          width='48%'
          size='xs'
        >
          Import Configs
        </Button>
      </Flex>
    </Flex>
  )
}

export default Footer

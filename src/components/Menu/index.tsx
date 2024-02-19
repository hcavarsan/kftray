import React, { useEffect, useState } from 'react'
import { MdAdd, MdFileDownload, MdFileUpload, MdMoreVert } from 'react-icons/md'

import {
  Box,
  Button,
  Grid,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Text,
  useColorModeValue,
} from '@chakra-ui/react'
import { app } from '@tauri-apps/api'

const MenuOptions = ({
  openModal,
  openSettingsModal,
  handleExportConfigs,
  handleImportConfigs,
}) => {
  const [version, setVersion] = useState('')
  const [isImportSubmenuOpen, setIsImportSubmenuOpen] = useState(false)

  useEffect(() => {
    app.getVersion().then(setVersion).catch(console.error)
  }, [])

  const handleSubmenuOpen = () => setIsImportSubmenuOpen(true)
  const handleSubmenuClose = () => setIsImportSubmenuOpen(false)

  return (
    <Box justifyContent='space-between'>
      <Grid templateColumns='repeat(2, 1fr)' gap={300} width='100%' mt={4}>
        <Menu placement='top'>
          <MenuButton
            as={Button}
            rightIcon={<MdMoreVert />}
            size='xs'
            colorScheme='facebook'
            variant='outline'
            borderRadius='md'
            width='85px'
          >
            Options
          </MenuButton>
          <MenuList onMouseLeave={handleSubmenuClose}>
            <MenuItem icon={<MdAdd />} onClick={openModal}>
              Add New Config
            </MenuItem>
            <MenuItem icon={<MdFileUpload />} onClick={handleExportConfigs}>
              Export Configs
            </MenuItem>
            <Box
              onMouseEnter={handleSubmenuOpen}
              onMouseLeave={handleSubmenuClose}
            >
              <Popover
                isOpen={isImportSubmenuOpen}
                placement='right-start'
                closeOnBlur={false}
              >
                <PopoverTrigger>
                  <MenuItem icon={<MdFileDownload />}>Import Configs</MenuItem>
                </PopoverTrigger>
                <PopoverContent
                  border='0'
                  boxShadow='none'
                  bg='transparent'
                  width='auto'
                >
                  <MenuList
                    onMouseEnter={handleSubmenuOpen}
                    onMouseLeave={handleSubmenuClose}
                  >
                    <MenuItem onClick={handleImportConfigs}>
                      From Local File
                    </MenuItem>
                    <MenuItem onClick={openSettingsModal}>From Git</MenuItem>
                  </MenuList>
                </PopoverContent>
              </Popover>
            </Box>
          </MenuList>
        </Menu>
        <Text
          fontSize='sm'
          textAlign='right'
          color={useColorModeValue('gray.500', 'gray.200')}
          fontFamily='Inter, sans-serif'
          p={2}
        >
          {version}
        </Text>
      </Grid>
    </Box>
  )
}

export default MenuOptions

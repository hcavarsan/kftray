import React, { useEffect, useState } from 'react'
import {
  MdAdd,
  MdAdminPanelSettings,
  MdFileDownload,
  MdFileUpload,
  MdMoreVert,
} from 'react-icons/md'

import {
  Box,
  Button,
  Grid,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Text,
  useColorModeValue,
} from '@chakra-ui/react'
import { app } from '@tauri-apps/api'

import { MenuProps } from '../../types'

const MenuOptions: React.FC<MenuProps> = ({
  openModal,
  openSettingsModal,
  handleExportConfigs,
  handleImportConfigs,
}) => {
  const [version, setVersion] = useState('')

  useEffect(() => {
    app.getVersion().then(setVersion)
  }, [])

  return (
    <Box justifyContent='space-between' mt='100' height='auto'>
      <Grid templateColumns='repeat(2, 1fr)' gap={300} width='100%'>
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

          <MenuList zIndex='popover'>
            <MenuItem icon={<MdAdd />} onClick={openModal}>
              Add New Config
            </MenuItem>
            <MenuItem icon={<MdFileUpload />} onClick={handleExportConfigs}>
              Export Configs
            </MenuItem>
            <MenuItem icon={<MdFileDownload />} onClick={handleImportConfigs}>
              Import Configs
            </MenuItem>
            <MenuItem
              icon={<MdAdminPanelSettings />}
              onClick={openSettingsModal}
            >
              Settings
            </MenuItem>
          </MenuList>
        </Menu>
        <Text
          fontSize='sm'
          textAlign='right'
          color='gray.400'
          fontFamily='Inter, sans-serif'
          p={2}
          borderColor={useColorModeValue('gray.200', 'gray.700')}
        >
          {version}
        </Text>
      </Grid>
    </Box>
  )
}

export default MenuOptions

import React, { useState } from 'react'
import { IoSettingsOutline } from 'react-icons/io5'
import { MdAdd, MdFileDownload, MdFileUpload } from 'react-icons/md'

import {
  Box,
  IconButton,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@chakra-ui/react'

import { MenuProps } from '../../types'

const MenuOptions: React.FC<MenuProps> = ({
  openModal,
  openSettingsModal,
  handleExportConfigs,
  handleImportConfigs,
}) => {
  const [isImportSubmenuOpen, setIsImportSubmenuOpen] = useState(false)

  const handleSubmenuOpen = () => setIsImportSubmenuOpen(true)
  const handleSubmenuClose = () => setIsImportSubmenuOpen(false)

  return (
    <Box display='flex' justifyContent='flex-start' width='100%' mt={4}>
      <Menu placement='top'>
        <MenuButton
          as={IconButton}
          aria-label='Options'
          icon={<IoSettingsOutline />}
          size='sm'
          colorScheme='facebook'
          variant='outline'
        />
        <MenuList onMouseLeave={handleSubmenuClose} zIndex='popover'>
          <MenuItem icon={<MdAdd />} onClick={openModal}>
            Add New Config
          </MenuItem>
          <MenuItem icon={<MdFileUpload />} onClick={handleExportConfigs}>
            Export Configs
          </MenuItem>
          <Box
            onMouseEnter={handleSubmenuOpen}
            onMouseLeave={handleSubmenuClose}
            position='relative'
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
                zIndex={1}
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
    </Box>
  )
}

export default MenuOptions

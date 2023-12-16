import { Box, Button, Flex, Stack, Table, TableContainer, Tbody, Th, Thead, Tr } from "@chakra-ui/react"
import { PortFoward } from "./portforward"
import { MdClose, MdRefresh } from "react-icons/md"

const PortForwardTable = (props) => {

    const {
        headerColor,
        configs,
        isInitiating,
        isStopping,
        isPortForwarding,
        initiatePortForwarding,
        stopPortForwarding,
        confirmDeleteConfig,
        handleDeleteConfig,
        handleEditConfig,
        isAlertOpen,
        setIsAlertOpen,
    } = props

    return (
        <>
        <Stack
            direction="row"
            spacing={2}
            justify="center"
            marginTop={0}
            marginBottom={4}
          >
            <Button
              leftIcon={<MdRefresh />}
              colorScheme="facebook"
              isLoading={isInitiating}
              loadingText="Starting..."
              onClick={initiatePortForwarding}
              isDisabled={isPortForwarding}
            >
              Start Forward
            </Button>
            <Button
              leftIcon={<MdClose />}
              colorScheme="facebook"
              isLoading={isStopping}
              loadingText="Stopping..."
              onClick={stopPortForwarding}
              isDisabled={!isPortForwarding}
            >
              Stop Forward
            </Button>
          </Stack>
            <TableContainer
              width="62vh"
              height="100%"
              maxHeight="400px"
              
              overflowY="auto"
              overflowX="hidden"
              display="block"

            >
            <Table variant="simple" size="sm" className="table-tiny">
              <Thead position="sticky" top={0} bgColor={headerColor} zIndex={99}>
                <Tr>
                  <Th>Service</Th>
                  <Th>Context</Th>
                  <Th>Namespace</Th>
                  <Th>Local Port</Th>
                  <Th>Status</Th>
                  <Th>Action</Th>
                </Tr>
              </Thead>
              <Tbody>
                {configs.map((config) => (
                  <PortFoward
                    key={config.id}
                    config={config}
                    confirmDeleteConfig={confirmDeleteConfig}
                    handleDeleteConfig={handleDeleteConfig}
                    handleEditConfig={handleEditConfig}
                    isAlertOpen={isAlertOpen}
                    setIsAlertOpen={setIsAlertOpen}
                    />
                ))}
              </Tbody>
            </Table>
            </TableContainer>
          </>
    )
}

export {
    PortForwardTable
}
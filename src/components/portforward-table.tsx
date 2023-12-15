import { Box, Button, Stack, Table, Tbody, Th, Thead, Tr } from "@chakra-ui/react"
import { PortFoward } from "./portforward"
import { MdClose, MdRefresh } from "react-icons/md"

const PortForwardTable = (props) => {

    const {
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
            spacing={4}
            justify="center"
            marginTop={0}
            mb={4}
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

          {/* Set the Table head outside of the scrollable body */}
          <Box width="100%" mt={0} p={0} borderRadius="10px">
            <Table variant="simple" size="sm">
              <Thead>
                <Tr>
                  <Th width="20%">Service</Th>
                  <Th width="25%">Context</Th>
                  <Th width="25%">Namespace</Th>
                  <Th width="20%">Local Port</Th>
                  <Th width="5%">Status</Th>
                  <Th width="5%">Action</Th>
                </Tr>
              </Thead>
            </Table>
          </Box>
          <Box
            width="100%"
            height="100%"
            overflowX="hidden"
            overflowY="auto"
            borderRadius="10px"

          >
            <Table variant="simple" size="sm" colorScheme="gray">
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
          </Box>
          </>
    )
}

export {
    PortForwardTable
}
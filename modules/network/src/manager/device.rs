use crate::{
    ADD_DNS_SERVER, ADD_IP_ADDRESS, ADD_ROUTE, GET_DHCP_STATE, GET_DNS_SERVER,
    GET_DNS_SERVER_COUNT, GET_HARDWARE_ADDRESS, GET_IP_ADDRESS, GET_IP_ADDRESS_COUNT,
    GET_MAXIMUM_BURST_SIZE, GET_MAXIMUM_TRANSMISSION_UNIT, GET_ROUTE, GET_ROUTE_COUNT, GET_STATE,
    IS_LINK_UP, IpAddress, IpCidr, MacAddress, REMOVE_DNS_SERVER, REMOVE_IP_ADDRESS, REMOVE_ROUTE,
    Route, SET_DHCP_STATE, SET_HARDWARE_ADDRESS, SET_STATE,
    manager::stack::{Stack, StackInner},
};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice, Error,
    MountOperations, Result,
};
use shared::AnyByLayout;

#[repr(transparent)]
pub struct NetworkDevice {
    stack: Stack,
}

impl NetworkDevice {
    pub fn with<R, F: FnOnce(&StackInner) -> R>(&self, f: F) -> Result<R> {
        let stack = self.stack.try_lock().ok_or(Error::RessourceBusy)?;

        Ok(f(&stack))
    }

    pub fn with_mut<R, F: FnOnce(&mut StackInner) -> R>(&self, f: F) -> Result<R> {
        let mut stack = self.stack.try_lock().ok_or(Error::RessourceBusy)?;

        Ok(f(&mut stack))
    }
}

impl NetworkDevice {
    pub fn new(stack: Stack) -> NetworkDevice {
        NetworkDevice { stack }
    }
}

impl DirectBaseOperations for NetworkDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            match command {
                SET_STATE::IDENTIFIER => {
                    let state: &bool = SET_STATE::cast_input(input)?;
                    self.with_mut(|s| s.set_state(*state))?;
                }
                GET_STATE::IDENTIFIER => {
                    let state: &mut bool = GET_STATE::cast_output(output)?;
                    *state = self.with(|s| s.get_state())?;
                }
                IS_LINK_UP::IDENTIFIER => {
                    let is_up: &mut bool = IS_LINK_UP::cast_output(output)?;
                    *is_up = self.with(|s| s.is_link_up())?;
                }
                GET_HARDWARE_ADDRESS::IDENTIFIER => {
                    let hardware_address: &mut MacAddress =
                        GET_HARDWARE_ADDRESS::cast_output(output)?;
                    let address = self.with(|s| s.interface.hardware_addr())?;
                    hardware_address.copy_from_slice(address.as_bytes());
                }
                SET_HARDWARE_ADDRESS::IDENTIFIER => {
                    let hardware_address: &MacAddress = SET_HARDWARE_ADDRESS::cast_input(input)?;
                    self.with_mut(|s| s.set_hardware_address(hardware_address))?;
                }
                GET_MAXIMUM_TRANSMISSION_UNIT::IDENTIFIER => {
                    let mtu: &mut usize = GET_MAXIMUM_TRANSMISSION_UNIT::cast_output(output)?;
                    *mtu = self.with(|s| s.get_maximum_transmission_unit())?;
                }
                GET_MAXIMUM_BURST_SIZE::IDENTIFIER => {
                    let maximum_burst_size: &mut Option<usize> =
                        GET_MAXIMUM_BURST_SIZE::cast_output(output)?;
                    *maximum_burst_size = self.with(|s| s.get_maximum_burst_size())?;
                }
                GET_IP_ADDRESS_COUNT::IDENTIFIER => {
                    let ip_address_count: &mut usize = GET_IP_ADDRESS_COUNT::cast_output(output)?;
                    *ip_address_count = self.with(|s| s.get_ip_addresses_count())?;
                }
                GET_IP_ADDRESS::IDENTIFIER => {
                    let (input, output) = GET_IP_ADDRESS::cast(input, output)?;
                    *output = self
                        .with(|s| s.get_ip_address(*input))?
                        .ok_or(Error::InvalidParameter)?;
                }
                ADD_IP_ADDRESS::IDENTIFIER => {
                    let indexed: &IpCidr = ADD_IP_ADDRESS::cast_input(input)?;
                    self.with_mut(|s| s.add_ip_address(indexed.clone()))?
                        .map_err(|_| Error::InternalError)?;
                }
                REMOVE_IP_ADDRESS::IDENTIFIER => {
                    let ip_address_index: &usize = REMOVE_IP_ADDRESS::cast_input(input)?;
                    self.with_mut(|s| s.remove_ip_address(*ip_address_index))?;
                }
                GET_ROUTE_COUNT::IDENTIFIER => {
                    let route_count: &mut usize = GET_ROUTE_COUNT::cast_output(output)?;
                    *route_count = self.with_mut(|s| s.get_route_count())?;
                }
                GET_ROUTE::IDENTIFIER => {
                    let (input, output) = GET_ROUTE::cast(input, output)?;
                    *output = self
                        .with_mut(|s| s.get_route(*input))?
                        .ok_or(Error::InvalidParameter)?;
                }
                ADD_ROUTE::IDENTIFIER => {
                    let route_index: &Route = ADD_ROUTE::cast_input(input)?;
                    self.with_mut(|s| s.add_route(route_index.clone()))?
                        .map_err(|_| Error::InternalError)?;
                }
                REMOVE_ROUTE::IDENTIFIER => {
                    let route_index: &usize = REMOVE_ROUTE::cast_input(input)?;
                    self.with_mut(|s| s.remove_route(*route_index))?;
                }
                GET_DNS_SERVER_COUNT::IDENTIFIER => {
                    let dns_server_count: &mut usize = GET_DNS_SERVER_COUNT::cast_output(output)?;
                    *dns_server_count = self.with_mut(|s| s.get_dns_servers_count())?;
                }
                GET_DNS_SERVER::IDENTIFIER => {
                    let (input, output) = GET_DNS_SERVER::cast(input, output)?;

                    *output = self
                        .with_mut(|s| s.get_dns_server(*input))?
                        .ok_or(Error::InvalidParameter)?;
                }
                ADD_DNS_SERVER::IDENTIFIER => {
                    let dns_server: &IpAddress = ADD_DNS_SERVER::cast_input(input)?;
                    self.with_mut(|s| s.add_dns_server(dns_server.clone()))?
                        .map_err(|_| Error::InternalError)?;
                }
                REMOVE_DNS_SERVER::IDENTIFIER => {
                    let index: &usize = REMOVE_DNS_SERVER::cast_input(input)?;
                    self.with_mut(|s| s.remove_dns_server(*index))?;
                }
                SET_DHCP_STATE::IDENTIFIER => {
                    let dhcp_enabled: &bool = SET_DHCP_STATE::cast_input(input)?;
                    self.with_mut(|s| s.set_dhcp_state(*dhcp_enabled))?;
                }
                GET_DHCP_STATE::IDENTIFIER => {
                    let dhcp_enabled: &mut bool = GET_DHCP_STATE::cast_output(output)?;
                    *dhcp_enabled = self.with(|s| s.get_dhcp_state())?;
                }
                command => {
                    self.with(|s| s.controller.control_sync(command, input, output))??;
                }
            }

            Ok(())
        })
    }
}

impl MountOperations for NetworkDevice {}

impl DirectCharacterDevice for NetworkDevice {}

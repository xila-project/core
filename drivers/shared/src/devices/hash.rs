use device::hash::{self, HashAlgorithm, SET_ALGORITHM};
use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Result, Size,
};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512, Sha512_224, Sha512_256, digest::DynDigest};
use shared::AnyByLayout;

#[derive(Clone)]
struct HashDeviceContext {
    hasher: Box<dyn DynDigest>,
}

unsafe impl Send for HashDeviceContext {}
unsafe impl Sync for HashDeviceContext {}

impl HashDeviceContext {
    fn new(algorithm: HashAlgorithm) -> Result<Self> {
        let hasher: Box<dyn DynDigest> = match algorithm {
            HashAlgorithm::Sha224 => Box::new(Sha224::new()),
            HashAlgorithm::Sha256 => Box::new(Sha256::new()),
            HashAlgorithm::Sha384 => Box::new(Sha384::new()),
            HashAlgorithm::Sha512 => Box::new(Sha512::new()),
            HashAlgorithm::Sha512_224 => Box::new(Sha512_224::new()),
            HashAlgorithm::Sha512_256 => Box::new(Sha512_256::new()),
            _ => return Err(Error::InvalidParameter),
        };

        Ok(Self { hasher })
    }
}

pub struct HashDevice;

impl BaseOperations for HashDevice {
    fn open(&self, context: &mut Context) -> Result<()> {
        context.set_private_data(Box::new(HashDeviceContext::new(HashAlgorithm::Sha256)?));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        context.take_private_data_of_type::<HashDeviceContext>();
        Ok(())
    }

    fn read(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let hash_context = context
                .get_private_data_mutable_of_type::<HashDeviceContext>()
                .ok_or(file_system::Error::InvalidParameter)?;

            if buffer.len() < hash_context.hasher.output_size() {
                return Err(Error::InvalidParameter);
            }

            let result = hash_context.hasher.clone().finalize();
            let length = result.len();
            buffer[..length].copy_from_slice(&result);
            Ok(length)
        })
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let hash_context = context
                .get_private_data_mutable_of_type::<HashDeviceContext>()
                .ok_or(file_system::Error::InvalidParameter)?;

            hash_context.hasher.update(buffer);
            Ok(buffer.len())
        })
    }

    fn control(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        _: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            let hash_context = context
                .get_private_data_mutable_of_type::<HashDeviceContext>()
                .ok_or(file_system::Error::InvalidParameter)?;

            match command {
                hash::RESET::IDENTIFIER => {
                    hash_context.hasher.reset();
                    Ok(())
                }
                hash::SET_ALGORITHM::IDENTIFIER => {
                    let algorithm = SET_ALGORITHM::cast_input(input)?;

                    *hash_context = HashDeviceContext::new(*algorithm)?;
                    Ok(())
                }
                _ => Err(file_system::Error::UnsupportedOperation),
            }
        })
    }

    fn clone_context(&self, context: &Context) -> Result<Context> {
        let hash_context = context
            .get_private_data_of_type::<HashDeviceContext>()
            .ok_or(file_system::Error::InvalidParameter)?;

        Ok(Context::new(Some(hash_context.clone())))
    }
}

impl MountOperations for HashDevice {}

impl CharacterDevice for HashDevice {}

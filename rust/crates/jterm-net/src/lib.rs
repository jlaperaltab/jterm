//! Clientes de red de jTerm. Todo pasa por el trait [`transport::Transport`]:
//! en la app es `ReqwestTransport`; en las pruebas, `FakeTransport` (nunca se habla con AWS).

pub mod aws_client;
pub mod s3_client;
pub mod transport;
pub mod updater;

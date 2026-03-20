pub mod Node {
    pub mod MessageCenter {}
    pub mod FileHandler {
        pub mod FileHandleTask {
            pub fn run() {}
        }
    }
    pub mod ConnectionHandler {
        pub mod Sender {}
        pub mod Receiver {}
    }
    pub mod InterfaceHandler {}
}
fn main() {
    // crate::router::run();
    Node::MessageCenter::run();
    Node::FileHandler::FileHandleTask::run();
    Node::ConnectionHandler::Sender::run();
    Node::ConnectionHandler::Receiver::run();
    Node::InterfaceHandler::run();
}

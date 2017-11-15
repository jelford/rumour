
error_chain! {
    errors {
        BadPortSpecified {
            description("Bad port specified")
            display("Bad port specified")
        }
        BadClusterJoinAddressSpecified {
            description("Bad peer address specified")
            display("Bad peer address specified")

        }
    }

    foreign_links {
        Io(::std::io::Error);
    }
}

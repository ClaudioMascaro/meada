macro_rules! cfg_runtime {
    ($($item:item)*) => {
        $(
            #[cfg(feature = "rt")]
            #[cfg_attr(docsrs, doc(cfg(feature = "rt")))]
            $item
        )*
    }
}

macro_rules! cfg_net {
    ($($item:item)*) => {
        $( #[cfg(feature = "net")] $item )*
    }
}

macro_rules! wrapped {
    ($($item:item)*) => {
        mod hidden { $($item)* }
    }
}

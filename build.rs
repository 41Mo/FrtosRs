use std::env;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    #[cfg(feature = "matekh743")]
    std::fs::copy("libraries/boards/src/MatekH743/memory.x", std::path::PathBuf::from(out_dir.as_str()).join("memory.x"),).unwrap();
    #[cfg(feature = "matekf411")]
    std::fs::copy("libraries/boards/src/MatekF411/memory.x", std::path::PathBuf::from(out_dir.as_str()).join("memory.x"),).unwrap();

    let mut b = freertos_cargo_build::Builder::new();

    // Path to FreeRTOS kernel or set ENV "FREERTOS_SRC" instead
    b.freertos("FreeRTOS-Kernel");
    b.freertos_config("src");       // Location of `FreeRTOSConfig.h` 

    #[cfg(feature = "matekh743")]
    b.freertos_port("GCC/ARM_CM7/r0p1".to_string()); // Port dir relativ to 'FreeRTOS-Kernel/portable' 
    // b.freertos_port("GCC/ARM_CM4F".to_string()); // Port dir relativ to 'FreeRTOS-Kernel/portable' 
    
                                                     //
    // #[cfg(feature = "matekf411")]
    // b.freertos_port("GCC/ARM_CM7/r0p1".to_string()); // Port dir relativ to 'FreeRTOS-Kernel/portable' 
    // b.heap("heap_4.c");             // Set the heap_?.c allocator to use from 
                                    // 'FreeRTOS-Kernel/portable/MemMang' (Default: heap_4.c)       

    // b.get_cc().file("More.c");   // Optional additional C-Code to be compiled

    b.compile().unwrap_or_else(|e| { panic!("{}", e.to_string()) });
}

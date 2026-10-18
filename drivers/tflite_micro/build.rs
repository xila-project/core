fn main() {
    let root = "vendor/tflite-micro";
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-rtti")
        .define("TF_LITE_STATIC_MEMORY", None)
        .define("TF_LITE_STRIP_ERROR_STRINGS", None)
        .define("FLATBUFFERS_LOCALE_INDEPENDENT", "0")
        .include(root)
        .include(format!("{root}/third_party/flatbuffers/include"))
        .file("src/tflm_wrapper.cc");
    for source in [
        "tensorflow/lite/micro/flatbuffer_conversions.cc",
        "tensorflow/lite/micro/flatbuffer_utils.cc",
        "tensorflow/lite/micro/memory_helpers.cc",
        "tensorflow/lite/micro/micro_allocation_info.cc",
        "tensorflow/lite/micro/micro_allocator.cc",
        "tensorflow/lite/micro/micro_common.cc",
        "tensorflow/lite/micro/micro_context.cc",
        "tensorflow/lite/micro/micro_interpreter.cc",
        "tensorflow/lite/micro/micro_interpreter_context.cc",
        "tensorflow/lite/micro/micro_interpreter_graph.cc",
        "tensorflow/lite/micro/micro_op_resolver.cc",
        "tensorflow/lite/micro/micro_resource_variable.cc",
        "tensorflow/lite/micro/micro_utils.cc",
        "tensorflow/lite/micro/arena_allocator/non_persistent_arena_buffer_allocator.cc",
        "tensorflow/lite/micro/arena_allocator/persistent_arena_buffer_allocator.cc",
        "tensorflow/lite/micro/arena_allocator/single_arena_buffer_allocator.cc",
        "tensorflow/lite/micro/memory_planner/greedy_memory_planner.cc",
        "tensorflow/lite/micro/memory_planner/linear_memory_planner.cc",
        "tensorflow/lite/micro/kernels/add.cc",
        "tensorflow/lite/micro/kernels/add_common.cc",
        "tensorflow/lite/micro/kernels/kernel_util.cc",
        "tensorflow/lite/micro/kernels/internal/quantization_util.cc",
    ] {
        build.file(format!("{root}/{source}"));
    }
    // Vendored files are the complete input: the build never downloads sources.
    println!("cargo:rerun-if-changed=vendor/tflite-micro");
    println!("cargo:rerun-if-changed=src/tflm_wrapper.cc");
    println!("cargo:rerun-if-changed=src/tflm_wrapper.h");
    build.warnings(false).compile("xila_tflm");
}

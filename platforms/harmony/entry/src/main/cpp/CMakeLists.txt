cmake_minimum_required(VERSION 3.5.0)
project(fdxmp_ohos_bridge)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# HarmonyOS 工具链会注入 OHOS_ARCH（arm64-v8a / x86_64 等）。
if(NOT DEFINED OHOS_ARCH)
    set(OHOS_ARCH "arm64-v8a")
endif()

# Rust 侧（crates/ffi）交叉编译产物，按 ABI 分目录存放。
set(FDXMP_LIB_DIR ${CMAKE_CURRENT_SOURCE_DIR}/libs/${OHOS_ARCH})

add_library(fdxmp_ohos_bridge SHARED napi_init.cpp)

target_include_directories(fdxmp_ohos_bridge PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})

target_link_libraries(fdxmp_ohos_bridge PUBLIC
    libace_napi.z.so
    ${FDXMP_LIB_DIR}/libfdxmp_ffi.so
)

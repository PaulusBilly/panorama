{
  "targets": [
    {
      "target_name": "mpv_host",
      "sources": [],
      "include_dirs": [
        "<!@(node -p \"require('node-addon-api').include\")"
      ],
      "dependencies": ["<!(node -p \"require('node-addon-api').gyp\")"],
      "defines": ["NAPI_CPP_EXCEPTIONS"],
      "conditions": [
        ["OS==\"mac\"", {
          "sources": ["src/addon.mm"],
          "include_dirs": ["<(module_root_dir)/../../../.cache/panorama/macos-libmpv/current/include"],
          "xcode_settings": {
            "CLANG_CXX_LANGUAGE_STANDARD": "c++20",
            "CLANG_ENABLE_OBJC_ARC": "YES",
            "GCC_ENABLE_CPP_EXCEPTIONS": "YES",
            "MACOSX_DEPLOYMENT_TARGET": "12.0",
            "OTHER_CFLAGS": ["-Wno-deprecated-declarations"],
            "OTHER_LDFLAGS": [
              "-framework AppKit",
              "-framework CoreVideo",
              "-framework CoreAudio",
              "-framework OpenGL",
              "-framework QuartzCore",
              "-L<(module_root_dir)/../../../.cache/panorama/macos-libmpv/current/lib",
              "-lmpv",
              "-Wl,-rpath,<(module_root_dir)/../../../.cache/panorama/macos-libmpv/current/lib"
            ]
          }
        }],
        ["OS==\"win\"", {
          "sources": ["src/addon_win.cc"],
          "include_dirs": ["<(module_root_dir)/../../../.cache/panorama/windows-libmpv/current/include"],
          "libraries": ["user32.lib", "ole32.lib", "ksuser.lib"],
          "win_delay_load_hook": "true",
          "msvs_settings": {
            "VCCLCompilerTool": {
              "AdditionalOptions": ["/std:c++20"],
              "ExceptionHandling": 1,
              "WarningLevel": 4
            }
          }
        }]
      ]
    }
  ]
}

cmake_minimum_required(VERSION 3.16)
project(embedide_native_smoke LANGUAGES C)
add_executable(blink-host ../make-smoke/main.c)

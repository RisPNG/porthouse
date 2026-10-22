# Reads the version from the workspace manifest so there is one place to change it.
find_package(Python3 3.11 REQUIRED COMPONENTS Interpreter)
execute_process(
    COMMAND "${Python3_EXECUTABLE}" -c "import sys, tomllib; print(tomllib.load(open(sys.argv[1], 'rb'))['workspace']['package']['version'])"
            "${CMAKE_CURRENT_LIST_DIR}/../Cargo.toml"
    OUTPUT_VARIABLE PORTHOUSE_VERSION
    OUTPUT_STRIP_TRAILING_WHITESPACE
    COMMAND_ERROR_IS_FATAL ANY
)

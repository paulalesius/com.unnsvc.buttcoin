#----------------------------------------------------------------
# Generated CMake target import file for configuration "Debug".
#----------------------------------------------------------------

# Commands may need to know the format version.
set(CMAKE_IMPORT_FILE_VERSION 1)

# Import target "leveldb::leveldb" for configuration "Debug"
set_property(TARGET leveldb::leveldb APPEND PROPERTY IMPORTED_CONFIGURATIONS DEBUG)
set_target_properties(leveldb::leveldb PROPERTIES
  IMPORTED_LINK_INTERFACE_LANGUAGES_DEBUG "CXX"
  IMPORTED_LOCATION_DEBUG "/usr/src/com.unnsvc/com.unnsvc.buttcoin/bitcoindb/target/debug/build/leveldb-sys-b3eebc4395745c99/out/lib/libleveldb.a"
  )

list(APPEND _IMPORT_CHECK_TARGETS leveldb::leveldb )
list(APPEND _IMPORT_CHECK_FILES_FOR_leveldb::leveldb "/usr/src/com.unnsvc/com.unnsvc.buttcoin/bitcoindb/target/debug/build/leveldb-sys-b3eebc4395745c99/out/lib/libleveldb.a" )

# Commands beyond this point should not need to know the version.
set(CMAKE_IMPORT_FILE_VERSION)

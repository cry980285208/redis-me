@echo off
rem RedisME custom codec launcher (Redisson serialized value <===> editable JSON).
rem Codec is auto-detected (Kryo5Codec -> MarshallingCodec), works with
rem Redisson 3.16+ / 4.x depending on the jars in the lib folder.
rem
rem Setup:
rem   1. Copy your project's runtime dependency jars into:  %~dp0lib
rem      (Maven one-liner in your project:
rem         mvn dependency:copy-dependencies -DoutputDirectory=lib )
rem      Required at least: redisson, kryo (or jboss-marshalling for 3.x default),
rem      netty-buffer, netty-common, jackson-databind/core/annotations,
rem      objenesis, slf4j-api.
rem   2. Copy your business classes (with package dirs) into:  %~dp0lib\classes
rem   3. In RedisME "Settings -> Custom Codec", set the command to this script path.
rem
rem Optional: set env REDISSON_CODEC_CLASS to the codec class configured in
rem your Redisson Config (skips auto-detection), e.g. org.redisson.codec.JsonJacksonCodec.

setlocal
set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

rem ---- locate java.exe: JAVA_HOME first, then PATH ----
set "JAVA_EXE="
if defined JAVA_HOME set "JAVA_EXE=%JAVA_HOME%\bin\java.exe"
if not exist "%JAVA_EXE%" set "JAVA_EXE="
if not defined JAVA_EXE (
    for /f "delims=" %%i in ('where java.exe 2^>nul') do if not defined JAVA_EXE set "JAVA_EXE=%%i"
)
if not defined JAVA_EXE (
    echo [redisson-codec] java.exe not found. Set JAVA_HOME or add java to PATH. 1>&2
    exit /b 1
)

set "CP=%SCRIPT_DIR%\redisson-codec.jar;%SCRIPT_DIR%\lib\*;%SCRIPT_DIR%\lib\classes"
"%JAVA_EXE%" -cp "%CP%" com.redisme.codec.RedissonCodec %*

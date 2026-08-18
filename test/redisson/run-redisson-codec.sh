#!/bin/sh
# RedisME 自定义编解码启动脚本（Redisson 序列化值 <-> 可编辑 JSON）
# codec 自动探测（Kryo5Codec -> MarshallingCodec），随 lib 中的 jar
# 兼容 Redisson 3.16+ / 4.x。
#
# 使用前准备：
#   1. 把项目的运行依赖 jar 全部放入本脚本同级的 lib 目录
#      （至少需要：redisson、kryo（3.x 默认则为 jboss-marshalling）、netty-buffer、
#        netty-common、jackson-databind/core/annotations、objenesis、slf4j-api；
#        建议直接把全部依赖 jar 拷入，Maven 一键命令：
#        mvn dependency:copy-dependencies -DoutputDirectory=<lib 目录>）
#   2. 把项目的业务 classes 放入 lib/classes（保留包目录结构）
#   3. 在 RedisME「设置 -> 自定义编解码」中，把命令填为本脚本路径
#
# 可选：环境变量 REDISSON_CODEC_CLASS 指定 Redisson Config 中实际配置的
# codec 类名（跳过自动探测），如 org.redisson.codec.JsonJacksonCodec

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# ---- 定位 java：优先 JAVA_HOME，其次 PATH ----
JAVA_EXE=""
if [ -n "$JAVA_HOME" ] && [ -x "$JAVA_HOME/bin/java" ]; then
    JAVA_EXE="$JAVA_HOME/bin/java"
elif command -v java >/dev/null 2>&1; then
    JAVA_EXE=$(command -v java)
else
    echo "[redisson-codec] 未找到 java，请设置 JAVA_HOME 或将 java 加入 PATH" >&2
    exit 1
fi

CP="$SCRIPT_DIR/redisson-codec.jar:$SCRIPT_DIR/lib/*:$SCRIPT_DIR/lib/classes"
exec "$JAVA_EXE" -cp "$CP" com.redisme.codec.RedissonCodec "$@"

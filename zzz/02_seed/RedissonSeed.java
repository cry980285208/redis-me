import com.redisme.demo.Person;
import io.netty.buffer.ByteBuf;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.File;
import java.io.IOException;
import java.math.BigDecimal;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import org.redisson.client.protocol.Decoder;
import org.redisson.client.protocol.Encoder;
import org.redisson.codec.Kryo5Codec;

/**
 * 向 Redis 写入两批 Redisson 样例，供 RedisME 自定义编码 Auto 识别验证。
 *
 * <ul>
 *   <li>{@code encoding:redisson-kryo5:} — Redisson 4.x 默认 Kryo5Codec
 *   <li>{@code encoding:redisson-marshalling:} — Redisson 3.x 默认 MarshallingCodec（4.x 已移除）
 * </ul>
 *
 * <p>两者都没有 JDK 序列化魔数，内置 Auto 会落到 Hex，再交给勾了 Auto 的脚本。连接：命令行 &gt;
 * REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD &gt; 127.0.0.1:6379。编解码目录：REDISSON_CODEC_HOME，默认
 * {@code C:/Users/he_pe/redis/custom/redisson}。{@code lib} 里已有 {@code redisson-marshalling-codec.jar}
 *（只含 MarshallingCodec）和 jboss-marshalling，脚本会先试 Kryo5，回环对不上再试 Marshalling。
 *
 * <pre>
 *   set HOME=C:\Users\he_pe\redis\custom\redisson
 *   javac --release 11 -cp "%HOME%\lib\*" -d %HOME%\target-seed RedissonSeed.java
 *   java -cp "%HOME%\target-seed;%HOME%\lib\*;%HOME%\lib\classes" RedissonSeed
 * </pre>
 *
 * <p>STRING 整键勾选该自定义编码的 Auto 后，旁注是设置里的编码名，正文是 JSON。每批的 {@code plain-utf8}
 * 仍是 UTF8，{@code not-redisson} 应保持 Hex。Hash/List/Set/ZSet 打开字段编辑，当前字段可识别（混有一条 UTF-8）。
 */
public class RedissonSeed {

  public static void main(String[] args) throws Exception {
    String host = args.length > 0 ? args[0] : envOr("REDIS_SERVER", "127.0.0.1");
    int port =
        args.length > 1 ? Integer.parseInt(args[1]) : Integer.parseInt(envOr("REDIS_PROT", "6379"));
    String password = args.length > 2 ? args[2] : envOr("REDIS_PASSWORD", "hepengju");

    try (RedisCli redis = new RedisCli(host, port)) {
      if (password != null && !password.isEmpty()) {
        redis.auth(password);
      }
      deleteLegacy(redis);
      Kryo5Codec kryo = new Kryo5Codec();
      int kryoCount =
          seedBatch(
              redis,
              "encoding:redisson-kryo5:",
              kryo.getValueEncoder(),
              kryo.getValueDecoder());
      LoadedCodec marshalling = marshallingCodec();
      int marshallingCount =
          seedBatch(
              redis,
              "encoding:redisson-marshalling:",
              marshalling.encoder,
              marshalling.decoder);
      System.out.printf(
          "done. kryo5 %d + marshalling %d STRING keys, each with hash/list/set/zset + 2 controls.%n",
          kryoCount, marshallingCount);
    }
  }

  /** 上一批单前缀 encoding:redisson: 的键，避免和两批新键混在一起。 */
  static void deleteLegacy(RedisCli redis) throws IOException {
    for (String suffix : suffixes()) {
      redis.del("encoding:redisson:" + suffix);
    }
  }

  static int seedBatch(RedisCli redis, String prefix, Encoder encoder, Decoder<Object> decoder)
      throws Exception {
    int ok = 0;
    for (Map.Entry<String, Object> e : buildSamples(prefix).entrySet()) {
      byte[] payload = encode(encoder, e.getValue());
      decoder.decode(io.netty.buffer.Unpooled.wrappedBuffer(payload), null);
      String json = decodeWithCodec(payload);
      redis.set(e.getKey(), payload);
      ok++;
      if (e.getKey().endsWith("large-string") && !json.contains("慢识别")) {
        throw new IOException("large-string decode missing marker: " + json.substring(0, 80));
      }
      if (e.getKey().endsWith("large-bytes") && !json.contains("\"[B\"")) {
        throw new IOException("large-bytes decode missing byte array: " + json.substring(0, 80));
      }
      System.out.printf("SET %s (%d bytes)%n", e.getKey(), payload.length);
      System.out.println("  " + (json.length() > 160 ? json.substring(0, 160) + "…" : json));
    }
    seedCompound(redis, prefix, encoder);
    seedControls(redis, prefix);
    return ok;
  }

  static LoadedCodec marshallingCodec() throws Exception {
    Class<?> type = Class.forName("org.redisson.codec.MarshallingCodec");
    Object codec = type.getDeclaredConstructor().newInstance();
    Encoder encoder = (Encoder) type.getMethod("getValueEncoder").invoke(codec);
    @SuppressWarnings("unchecked")
    Decoder<Object> decoder = (Decoder<Object>) type.getMethod("getValueDecoder").invoke(codec);
    return new LoadedCodec(encoder, decoder);
  }

  static final class LoadedCodec {
    final Encoder encoder;
    final Decoder<Object> decoder;

    LoadedCodec(Encoder encoder, Decoder<Object> decoder) {
      this.encoder = encoder;
      this.decoder = decoder;
    }
  }

  static Map<String, Object> buildSamples(String prefix) {
    Map<String, Object> m = new LinkedHashMap<>();
    m.put(prefix + "string", "hello-redisson");
    m.put(prefix + "empty-string", "");
    m.put(prefix + "chinese", "张三，你好");
    m.put(prefix + "integer", Integer.valueOf(42));
    m.put(prefix + "long", Long.valueOf(9_876_543_210L));
    m.put(prefix + "double", Double.valueOf(3.14159));
    m.put(prefix + "boolean-true", Boolean.TRUE);
    m.put(prefix + "boolean-false", Boolean.FALSE);
    m.put(prefix + "bytes", new byte[] {0x01, 0x02, (byte) 0xff, 0x00, 0x7f});
    m.put(prefix + "uuid", UUID.fromString("550e8400-e29b-41d4-a716-446655440000"));
    m.put(prefix + "localdate", LocalDate.of(2024, 6, 1));
    m.put(prefix + "localdatetime", LocalDateTime.of(2024, 6, 1, 14, 30, 5));
    m.put(prefix + "instant", Instant.parse("2024-06-01T06:30:05Z"));
    m.put(prefix + "bigdecimal", new BigDecimal("12345.6789"));

    m.put(prefix + "arraylist", new ArrayList<>(Arrays.asList("a", "b", "中文")));
    m.put(prefix + "empty-list", new ArrayList<String>());
    m.put(prefix + "list-of", List.of("x", "y"));
    m.put(prefix + "string-array", new String[] {"one", "two"});
    m.put(prefix + "int-array", new int[] {1, 2, 3});

    Map<String, Object> map = new LinkedHashMap<>();
    map.put("name", "Alice");
    map.put("age", Integer.valueOf(30));
    map.put("ok", Boolean.TRUE);
    m.put(prefix + "linkedhashmap", map);
    m.put(prefix + "empty-map", new LinkedHashMap<String, Object>());

    Person p1 = new Person("1", "张三", 33);
    Person p2 = new Person("2", "李四", 44);
    m.put(prefix + "person", p1);
    m.put(prefix + "person-list", new ArrayList<>(Arrays.asList(p1, p2)));

    Map<String, Object> nested = new LinkedHashMap<>();
    nested.put("owner", p1);
    nested.put("tags", new ArrayList<>(Arrays.asList("red", "blue")));
    m.put(prefix + "nested", nested);

    // 约 100KB，走自定义编码的 stdin 参数，用来看识别偏慢时的 loading
    m.put(prefix + "large-string", largeText());
    m.put(prefix + "large-bytes", largeBytes());
    return m;
  }

  /** 开头带标记，后面用 ASCII 垫到约 100KB（UTF-8）。 */
  static String largeText() {
    StringBuilder sb = new StringBuilder(100 * 1024);
    sb.append("慢识别");
    while (sb.length() < 100 * 1024) {
      sb.append('A');
    }
    return sb.toString();
  }

  static byte[] largeBytes() {
    byte[] bytes = new byte[100 * 1024];
    for (int i = 0; i < bytes.length; i++) {
      bytes[i] = (byte) (i * 31 + 7);
    }
    return bytes;
  }

  static List<String> suffixes() {
    List<String> names = new ArrayList<>(buildSamples("").keySet());
    names.add("hash");
    names.add("list-key");
    names.add("set-key");
    names.add("zset");
    names.add("plain-utf8");
    names.add("not-redisson");
    return names;
  }

  static void seedCompound(RedisCli redis, String prefix, Encoder encoder) throws Exception {
    Person user = new Person("1001", "Alice", 28);
    byte[] userBytes = encode(encoder, user);
    byte[] listBytes = encode(encoder, new ArrayList<>(Arrays.asList("a", "b", "中文")));
    byte[] dateBytes = encode(encoder, LocalDate.of(2024, 6, 1));

    String hashKey = prefix + "hash";
    redis.del(hashKey);
    redis.hset(hashKey, "user", userBytes);
    redis.hset(hashKey, "list", listBytes);
    redis.hset(hashKey, "localdate", dateBytes);
    redis.hset(hashKey, "plain-utf8", "新增字段".getBytes(StandardCharsets.UTF_8));
    System.out.printf("HSET %s (user/list/localdate=该编码, plain-utf8=UTF8)%n", hashKey);

    String listKey = prefix + "list-key";
    redis.del(listKey);
    redis.rpush(listKey, encode(encoder, "hello-list"));
    redis.rpush(listKey, encode(encoder, Integer.valueOf(42)));
    redis.rpush(listKey, userBytes);
    redis.rpush(listKey, "纯文本元素".getBytes(StandardCharsets.UTF_8));
    System.out.printf("RPUSH %s (3×该编码 + 1×UTF8)%n", listKey);

    String setKey = prefix + "set-key";
    redis.del(setKey);
    redis.sadd(setKey, encode(encoder, "member-a"));
    redis.sadd(setKey, dateBytes);
    redis.sadd(setKey, "utf8-member".getBytes(StandardCharsets.UTF_8));
    System.out.printf("SADD %s (2×该编码 + 1×UTF8)%n", setKey);

    String zsetKey = prefix + "zset";
    redis.del(zsetKey);
    redis.zadd(zsetKey, 1.0, encode(encoder, "z-low"));
    redis.zadd(zsetKey, 2.5, userBytes);
    redis.zadd(zsetKey, 9.0, "z-utf8".getBytes(StandardCharsets.UTF_8));
    System.out.printf("ZADD %s (2×该编码 + 1×UTF8)%n", zsetKey);
  }

  /** 对照：明文不应进自定义识别；随机字节应被脚本拒绝并保持 Hex。 */
  static void seedControls(RedisCli redis, String prefix) throws IOException {
    redis.set(prefix + "plain-utf8", "plain-utf8-hello".getBytes(StandardCharsets.UTF_8));
    System.out.printf("SET %splain-utf8 (UTF8，不应识别成该编码)%n", prefix);
    redis.set(prefix + "not-redisson", new byte[] {(byte) 0xde, (byte) 0xad, 0x00, 0x01, 0x02});
    System.out.printf("SET %snot-redisson (随机字节，应保持 Hex)%n", prefix);
  }

  static byte[] encode(Encoder encoder, Object value) throws IOException {
    ByteBuf buf = encoder.encode(value);
    try {
      byte[] out = new byte[buf.readableBytes()];
      buf.getBytes(buf.readerIndex(), out);
      return out;
    } finally {
      if (buf.refCnt() > 0) {
        buf.release();
      }
    }
  }

  /** 走 RedisME 同一入口，确认界面能解出 JSON。失败则中止，避免写入认不出的键。 */
  static String decodeWithCodec(byte[] payload) throws Exception {
    String b64 = Base64.getEncoder().encodeToString(payload);
    File home = new File(envOr("REDISSON_CODEC_HOME", "C:/Users/he_pe/redis/custom/redisson"));
    String javaExe = System.getProperty("java.home") + File.separator + "bin" + File.separator + "java";
    // 4.7 在前，保证 Kryo5 用当前 lib；3.52 只补 MarshallingCodec（4.x 已删除该类）
    String cp =
        "redisson-codec.jar"
            + File.pathSeparator
            + "lib/*"
            + File.pathSeparator
            + "lib/classes"
            + File.pathSeparator
            + marshallingClasspath();
    // 与 RedisME 一致：Base64 超过 8000 字符时改走 stdin，避免命令行装不下约 100KB 的值
    boolean stdin = b64.length() >= 8000;
    ProcessBuilder pb =
        new ProcessBuilder(
            javaExe,
            "-cp",
            cp,
            "com.redisme.codec.RedissonCodec",
            "decode",
            stdin ? "--stdin" : b64);
    pb.directory(home);
    pb.redirectErrorStream(true);
    Process p = pb.start();
    ByteArrayOutputStream sink = new ByteArrayOutputStream();
    Thread drain =
        new Thread(
            () -> {
              try {
                p.getInputStream().transferTo(sink);
              } catch (IOException ignored) {
                // 下面用退出码判断
              }
            });
    drain.start();
    if (stdin) {
      p.getOutputStream().write(b64.getBytes(StandardCharsets.US_ASCII));
      p.getOutputStream().write('\n');
      p.getOutputStream().close();
    }
    drain.join();
    byte[] out = sink.toByteArray();
    int code = p.waitFor();
    String text = new String(out, StandardCharsets.UTF_8).trim();
    if (code != 0) {
      throw new IOException("codec decode failed (" + code + "): " + text);
    }
    return text.replace('\n', ' ');
  }

  /** redisson 3.52 提供 MarshallingCodec；jboss-marshalling 是它的运行时依赖。 */
  static String marshallingClasspath() {
    String m2 = System.getProperty("user.home").replace('\\', '/') + "/.m2/repository";
    return m2
        + "/org/redisson/redisson/3.52.0/redisson-3.52.0.jar"
        + File.pathSeparator
        + m2
        + "/org/jboss/marshalling/jboss-marshalling/2.0.11.Final/jboss-marshalling-2.0.11.Final.jar"
        + File.pathSeparator
        + m2
        + "/org/jboss/marshalling/jboss-marshalling-river/2.0.11.Final/jboss-marshalling-river-2.0.11.Final.jar";
  }

  static String envOr(String name, String def) {
    String v = System.getenv(name);
    return (v == null || v.isEmpty()) ? def : v;
  }

  static final class RedisCli implements AutoCloseable {
    private final Socket socket;
    private final java.io.OutputStream out;
    private final DataInputStream in;

    RedisCli(String host, int port) throws IOException {
      socket = new Socket(host, port);
      out = socket.getOutputStream();
      in = new DataInputStream(socket.getInputStream());
    }

    void auth(String password) throws IOException {
      writeCommand("AUTH", password.getBytes(StandardCharsets.UTF_8));
      readOk();
    }

    void set(String key, byte[] value) throws IOException {
      writeCommand("SET", key.getBytes(StandardCharsets.UTF_8), value);
      readOk();
    }

    void del(String key) throws IOException {
      writeCommand("DEL", key.getBytes(StandardCharsets.UTF_8));
      readInteger();
    }

    void hset(String key, String field, byte[] value) throws IOException {
      writeCommand(
          "HSET",
          key.getBytes(StandardCharsets.UTF_8),
          field.getBytes(StandardCharsets.UTF_8),
          value);
      readInteger();
    }

    void rpush(String key, byte[] value) throws IOException {
      writeCommand("RPUSH", key.getBytes(StandardCharsets.UTF_8), value);
      readInteger();
    }

    void sadd(String key, byte[] member) throws IOException {
      writeCommand("SADD", key.getBytes(StandardCharsets.UTF_8), member);
      readInteger();
    }

    void zadd(String key, double score, byte[] member) throws IOException {
      writeCommand(
          "ZADD",
          key.getBytes(StandardCharsets.UTF_8),
          Double.toString(score).getBytes(StandardCharsets.US_ASCII),
          member);
      readInteger();
    }

    private void writeCommand(String cmd, byte[]... args) throws IOException {
      out.write(("*" + (1 + args.length) + "\r\n").getBytes(StandardCharsets.US_ASCII));
      writeBulk(cmd.getBytes(StandardCharsets.US_ASCII));
      for (byte[] arg : args) {
        writeBulk(arg);
      }
      out.flush();
    }

    private void writeBulk(byte[] data) throws IOException {
      out.write(("$" + data.length + "\r\n").getBytes(StandardCharsets.US_ASCII));
      out.write(data);
      out.write(new byte[] {'\r', '\n'});
    }

    private void readOk() throws IOException {
      String line = readLine();
      if (line.startsWith("+")) return;
      if (line.startsWith("-")) throw new IOException("Redis error: " + line.substring(1));
      throw new IOException("unexpected Redis reply: " + line);
    }

    private void readInteger() throws IOException {
      String line = readLine();
      if (line.startsWith(":")) return;
      if (line.startsWith("-")) throw new IOException("Redis error: " + line.substring(1));
      throw new IOException("unexpected Redis reply: " + line);
    }

    private String readLine() throws IOException {
      ByteArrayOutputStream buf = new ByteArrayOutputStream();
      int prev = -1;
      while (true) {
        int b = in.read();
        if (b < 0) throw new IOException("Redis connection closed");
        if (prev == '\r' && b == '\n') break;
        if (prev >= 0 && prev != '\r') buf.write(prev);
        else if (prev == '\r' && b != '\n') buf.write('\r');
        prev = b;
      }
      return buf.toString(StandardCharsets.UTF_8);
    }

    @Override
    public void close() throws IOException {
      socket.close();
    }
  }
}

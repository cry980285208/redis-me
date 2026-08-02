import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.ObjectOutputStream;
import java.io.OutputStream;
import java.io.Serializable;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.net.InetAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.sql.Timestamp;
import java.time.Duration;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.LocalTime;
import java.time.MonthDay;
import java.time.OffsetDateTime;
import java.time.OffsetTime;
import java.time.Period;
import java.time.Year;
import java.time.YearMonth;
import java.time.ZoneId;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.BitSet;
import java.util.Calendar;
import java.util.Date;
import java.util.EnumMap;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.LinkedList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.PriorityQueue;
import java.util.Set;
import java.util.Stack;
import java.util.TimeZone;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.UUID;
import java.util.Vector;
import java.util.concurrent.ConcurrentHashMap;

/**
 * 向 Redis 写入若干 JDK 序列化样例键，供 RedisME「JavaSerial」查看验证。
 *
 * <p>需 JDK 11+，无第三方依赖（纯 Socket + RESP）。连接参数优先级：命令行 &gt; 环境变量 &gt;
 * 默认本机。环境变量：{@code REDIS_SERVER} / {@code REDIS_PROT} / {@code REDIS_PASSWORD}。
 *
 * <pre>
 *   java JavaSerialSeed.java
 *   java JavaSerialSeed.java 127.0.0.1 6379 hepengju
 *   java JavaSerialSeed.java 127.0.0.1 6379 ""          # 无密码
 * </pre>
 *
 * <p>写入的键前缀均为 {@code encoding:javaserial:}。STRING 整键选 JavaSerial；
 * Hash/List/Set/ZSet 打开字段弹窗即可（可测字段级 Auto，并混有 UTF-8 字段）。
 */
public class JavaSerialSeed {

  private static final String PREFIX = "encoding:javaserial:";

  /** 命令行 > REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD > 本机默认 */
  static String envOr(String name, String def) {
    String v = System.getenv(name);
    return (v == null || v.isEmpty()) ? def : v;
  }

  public static void main(String[] args) throws Exception {
    String host = args.length > 0 ? args[0] : envOr("REDIS_SERVER", "127.0.0.1");
    int port =
        args.length > 1
            ? Integer.parseInt(args[1])
            : Integer.parseInt(envOr("REDIS_PROT", "6379"));
    String password = args.length > 2 ? args[2] : envOr("REDIS_PASSWORD", "hepengju");

    Map<String, Object> samples = buildSamples();
    try (RedisCli redis = new RedisCli(host, port)) {
      if (password != null && !password.isEmpty()) {
        redis.auth(password);
      }
      for (Map.Entry<String, Object> e : samples.entrySet()) {
        byte[] payload = javaSerialize(e.getValue());
        redis.set(e.getKey(), payload);
        System.out.printf("SET %s (%d bytes)%n", e.getKey(), payload.length);
      }
      seedCompoundTypes(redis);
    }
    System.out.println("done. STRING → JavaSerial；Hash/List/Set/ZSet → 打开字段查看。");
  }

  /** 覆盖已增强类型：包装类型 / java.time / record / POJO / 集合 / BitSet 等 */
  static Map<String, Object> buildSamples() {
    Map<String, Object> m = new LinkedHashMap<>();
    m.put(PREFIX + "string", "hello-javaserial");
    m.put(PREFIX + "integer", Integer.valueOf(42));
    m.put(PREFIX + "long", Long.valueOf(9_876_543_210L));
    m.put(PREFIX + "double", Double.valueOf(3.14159));
    m.put(PREFIX + "float", Float.valueOf(2.5f));
    m.put(PREFIX + "short", Short.valueOf((short) 7));
    m.put(PREFIX + "byte", Byte.valueOf((byte) 9));
    m.put(PREFIX + "character", Character.valueOf('中'));
    m.put(PREFIX + "boolean", Boolean.TRUE);
    m.put(PREFIX + "bigdecimal", new BigDecimal("12345.6789"));
    m.put(PREFIX + "biginteger", new BigInteger("98765432109876543210"));
    m.put(PREFIX + "uuid", UUID.fromString("550e8400-e29b-41d4-a716-446655440000"));
    m.put(PREFIX + "enum", DemoRole.ADMIN);
    m.put(PREFIX + "locale", Locale.SIMPLIFIED_CHINESE);

    m.put(PREFIX + "date", new Date(1_700_000_000_000L)); // 2023-11-14T22:13:20Z

    m.put(PREFIX + "localdate", LocalDate.of(2024, 6, 1));
    m.put(PREFIX + "localtime", LocalTime.of(14, 30, 0));
    m.put(PREFIX + "localdatetime", LocalDateTime.of(2024, 6, 1, 14, 30, 0));
    m.put(PREFIX + "instant", Instant.parse("2024-06-01T06:30:00Z"));
    m.put(PREFIX + "zoneddatetime", ZonedDateTime.of(2024, 6, 1, 14, 30, 0, 0, ZoneId.of("Asia/Shanghai")));
    m.put(PREFIX + "offsetdatetime", OffsetDateTime.of(2024, 6, 1, 14, 30, 0, 0, ZoneOffset.ofHours(8)));
    m.put(PREFIX + "offsettime", OffsetTime.of(14, 30, 0, 0, ZoneOffset.ofHours(8)));
    m.put(PREFIX + "duration", Duration.ofHours(1).plusMinutes(30).plusSeconds(5));
    m.put(PREFIX + "period", Period.of(-1, 2, -3));
    m.put(PREFIX + "year", Year.of(2024));
    m.put(PREFIX + "yearmonth", YearMonth.of(2024, 6));
    m.put(PREFIX + "monthday", MonthDay.of(6, 1));

    // Java 16+ record（流形态与普通 Serializable 字段一致）
    m.put(PREFIX + "record-point", new DemoPoint(3, 4));
    m.put(PREFIX + "record-person", new DemoPerson("Bob", 20, new DemoPoint(1, 2)));

    DemoUser user = new DemoUser();
    user.id = 1001;
    user.name = "Alice";
    user.active = true;
    user.birthday = LocalDate.of(1990, 1, 15);
    user.createdAt = LocalDateTime.of(2024, 6, 1, 10, 0, 0);
    user.role = DemoRole.USER;
    m.put(PREFIX + "user", user);

    m.put(PREFIX + "array-string", new String[] {"a", "b", "中文"});
    m.put(PREFIX + "array-int", new int[] {1, 2, 3});

    List<Object> list = new ArrayList<>();
    list.add("a");
    list.add("b");
    list.add("中文");
    list.add(Integer.valueOf(1));
    m.put(PREFIX + "list", list);

    List<String> linked = new LinkedList<>();
    linked.add("x");
    linked.add("y");
    m.put(PREFIX + "linkedlist", linked);

    Map<String, Object> map = new HashMap<>();
    map.put("k1", "v1");
    map.put("k2", Integer.valueOf(7));
    map.put("day", LocalDate.of(2024, 12, 25));
    m.put(PREFIX + "map", map);

    Set<String> set = new HashSet<>();
    set.add("s1");
    set.add("s2");
    m.put(PREFIX + "set", set);

    Map<String, Object> linkedMap = new LinkedHashMap<>();
    linkedMap.put("first", 1);
    linkedMap.put("second", "二");
    m.put(PREFIX + "linkedmap", linkedMap);

    Set<String> linkedSet = new LinkedHashSet<>();
    linkedSet.add("ls1");
    linkedSet.add("ls2");
    m.put(PREFIX + "linkedset", linkedSet);

    Map<String, Integer> treeMap = new TreeMap<>();
    treeMap.put("b", 2);
    treeMap.put("a", 1);
    m.put(PREFIX + "treemap", treeMap);

    Set<String> treeSet = new TreeSet<>();
    treeSet.add("ts2");
    treeSet.add("ts1");
    m.put(PREFIX + "treeset", treeSet);

    Map<String, Object> chm = new ConcurrentHashMap<>();
    chm.put("ck1", "cv1");
    chm.put("ck2", Integer.valueOf(9));
    m.put(PREFIX + "concurrentmap", chm);

    EnumMap<DemoRole, String> enumMap = new EnumMap<>(DemoRole.class);
    enumMap.put(DemoRole.ADMIN, "a");
    enumMap.put(DemoRole.USER, "u");
    m.put(PREFIX + "enummap", enumMap);
    m.put(PREFIX + "enumset", EnumSet.of(DemoRole.ADMIN, DemoRole.GUEST));

    Vector<String> vector = new Vector<>();
    vector.add("v1");
    vector.add("v2");
    m.put(PREFIX + "vector", vector);

    Stack<String> stack = new Stack<>();
    stack.push("s1");
    stack.push("s2");
    m.put(PREFIX + "stack", stack);

    ArrayDeque<String> deque = new ArrayDeque<>();
    deque.add("d1");
    deque.add("d2");
    m.put(PREFIX + "deque", deque);

    PriorityQueue<Integer> prio = new PriorityQueue<>();
    prio.add(3);
    prio.add(1);
    prio.add(2);
    m.put(PREFIX + "priorityqueue", prio);

    BitSet bitSet = new BitSet();
    bitSet.set(1);
    bitSet.set(3);
    bitSet.set(8);
    m.put(PREFIX + "bitset", bitSet);

    m.put(PREFIX + "stringbuilder", new StringBuilder("hello"));
    m.put(PREFIX + "stringbuffer", new StringBuffer("world"));

    try {
      m.put(PREFIX + "inet", InetAddress.getByName("127.0.0.1"));
    } catch (Exception ignored) {
      // skip if resolver fails
    }

    m.put(PREFIX + "sqldate", java.sql.Date.valueOf("2024-06-01"));
    m.put(PREFIX + "timestamp", Timestamp.valueOf("2024-06-01 14:30:00.123456789"));

    Calendar cal = Calendar.getInstance(TimeZone.getTimeZone("UTC"));
    cal.setTimeInMillis(1_700_000_000_000L);
    m.put(PREFIX + "calendar", cal);

    return m;
  }

  /**
   * Hash / List / Set / ZSet：字段值为 Java 序列化字节（混一条 UTF-8，测字段级 Auto）。
   * 键：{@code encoding:javaserial:hash|list-key|set-key|zset}
   */
  static void seedCompoundTypes(RedisCli redis) throws IOException {
    String hashKey = PREFIX + "hash";
    redis.del(hashKey);
    redis.hset(hashKey, "user", javaSerialize(userSample()));
    List<Object> fieldList = new ArrayList<>();
    fieldList.add("a");
    fieldList.add("b");
    fieldList.add("中文");
    redis.hset(hashKey, "list", javaSerialize(fieldList));
    redis.hset(hashKey, "localdate", javaSerialize(LocalDate.of(2024, 6, 1)));
    redis.hset(hashKey, "plain-utf8", "新增字段".getBytes(StandardCharsets.UTF_8));
    System.out.printf("HSET %s (user/list/localdate=JavaSerial, plain-utf8=UTF8)%n", hashKey);

    String listKey = PREFIX + "list-key";
    redis.del(listKey);
    redis.rpush(listKey, javaSerialize("hello-list"));
    redis.rpush(listKey, javaSerialize(Integer.valueOf(42)));
    redis.rpush(listKey, javaSerialize(userSample()));
    redis.rpush(listKey, "纯文本元素".getBytes(StandardCharsets.UTF_8));
    System.out.printf("RPUSH %s (3×JavaSerial + 1×UTF8)%n", listKey);

    String setKey = PREFIX + "set-key";
    redis.del(setKey);
    redis.sadd(setKey, javaSerialize("member-a"));
    redis.sadd(setKey, javaSerialize(LocalDate.of(2024, 12, 25)));
    redis.sadd(setKey, "utf8-member".getBytes(StandardCharsets.UTF_8));
    System.out.printf("SADD %s (2×JavaSerial + 1×UTF8)%n", setKey);

    String zsetKey = PREFIX + "zset";
    redis.del(zsetKey);
    redis.zadd(zsetKey, 1.0, javaSerialize("z-low"));
    redis.zadd(zsetKey, 2.5, javaSerialize(userSample()));
    redis.zadd(zsetKey, 9.0, "z-utf8".getBytes(StandardCharsets.UTF_8));
    System.out.printf("ZADD %s (2×JavaSerial + 1×UTF8)%n", zsetKey);
  }

  static DemoUser userSample() {
    DemoUser user = new DemoUser();
    user.id = 1001;
    user.name = "Alice";
    user.active = true;
    user.birthday = LocalDate.of(1990, 1, 15);
    user.createdAt = LocalDateTime.of(2024, 6, 1, 10, 0, 0);
    user.role = DemoRole.USER;
    return user;
  }

  static byte[] javaSerialize(Object obj) throws IOException {
    ByteArrayOutputStream bos = new ByteArrayOutputStream();
    try (ObjectOutputStream oos = new ObjectOutputStream(bos)) {
      oos.writeObject(obj);
    }
    return bos.toByteArray();
  }

  public enum DemoRole {
    ADMIN,
    USER,
    GUEST
  }

  public record DemoPoint(int x, int y) implements Serializable {}

  public record DemoPerson(String name, int age, DemoPoint home) implements Serializable {}

  /** 简单可序列化 POJO（含 LocalDate / LocalDateTime / enum 字段） */
  public static class DemoUser implements Serializable {
    private static final long serialVersionUID = 1L;
    public int id;
    public String name;
    public boolean active;
    public LocalDate birthday;
    public LocalDateTime createdAt;
    public DemoRole role;
  }

  /** 最小 RESP 客户端：AUTH / SET / DEL / HSET / RPUSH / SADD / ZADD 二进制值 */
  static final class RedisCli implements AutoCloseable {
    private final Socket socket;
    private final OutputStream out;
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
      out.write(('*' + Integer.toString(1 + args.length) + "\r\n").getBytes(StandardCharsets.US_ASCII));
      writeBulk(cmd.getBytes(StandardCharsets.US_ASCII));
      for (byte[] arg : args) {
        writeBulk(arg);
      }
      out.flush();
    }

    private void writeBulk(byte[] data) throws IOException {
      out.write(('$' + Integer.toString(data.length) + "\r\n").getBytes(StandardCharsets.US_ASCII));
      out.write(data);
      out.write(new byte[] {'\r', '\n'});
    }

    private void readOk() throws IOException {
      String line = readLine();
      if (line.startsWith("+")) {
        return;
      }
      if (line.startsWith("-")) {
        throw new IOException("Redis error: " + line.substring(1));
      }
      throw new IOException("unexpected Redis reply: " + line);
    }

    private void readInteger() throws IOException {
      String line = readLine();
      if (line.startsWith(":")) {
        return;
      }
      if (line.startsWith("-")) {
        throw new IOException("Redis error: " + line.substring(1));
      }
      throw new IOException("unexpected Redis reply: " + line);
    }

    private String readLine() throws IOException {
      ByteArrayOutputStream buf = new ByteArrayOutputStream();
      int prev = -1;
      while (true) {
        int b = in.read();
        if (b < 0) {
          throw new IOException("Redis connection closed");
        }
        if (prev == '\r' && b == '\n') {
          break;
        }
        if (prev >= 0 && prev != '\r') {
          buf.write(prev);
        } else if (prev == '\r' && b != '\n') {
          buf.write('\r');
        }
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

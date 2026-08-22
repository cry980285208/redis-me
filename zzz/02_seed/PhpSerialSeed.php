<?php
/**
 * 向 Redis 写入若干 PHP serialize 样例键，供 RedisME「PhpSerial」查看验证。
 *
 * 需本机 PHP 8.0+（无扩展依赖，纯 socket + RESP）。serialize() 原生输出即正确性基准，
 * 重点覆盖前端 php-serialize（binary 编码解析）的还原：s: 按字节计数、中文多字节、
 * O:/C: 未知类、"\0Class\0" 私有属性转义等。
 *
 * 连接参数优先级：命令行 > 环境变量 > 默认本机。
 * 环境变量：REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD。
 *
 * 用法::
 *
 *     php PhpSerialSeed.php
 *     php PhpSerialSeed.php 127.0.0.1 6379 hepengju
 *     php PhpSerialSeed.php 127.0.0.1 6379 ""          # 无密码
 *
 * 写入的键前缀均为 encoding:phpserial:。STRING 整键选 PhpSerial / Auto；
 * Hash/List/Set/ZSet 打开字段弹窗即可（可测字段级 Auto，并混有 UTF-8 字段）。
 */

const PREFIX = 'encoding:phpserial:';

// ---------- 样例对象定义 ----------

/** 简单公开属性对象（未知类 → 前端 $class 占位展示） */
class DemoPoint
{
    public function __construct(
        public int $x,
        public int $y,
    ) {}
}

/** 嵌套对象 + 中文属性值 */
class DemoPerson
{
    public function __construct(
        public string $name,
        public int $age,
        public DemoPoint $home,
    ) {}
}

/** 混合可见性：public / protected / private（序列化键带 "\0*\0" / "\0Class\0" 前缀） */
class DemoUser
{
    public int $id;
    public string $name;
    public bool $active;
    protected string $email;
    private string $secret;

    public function __construct(int $id, string $name, bool $active, string $email, string $secret)
    {
        $this->id = $id;
        $this->name = $name;
        $this->active = $active;
        $this->email = $email;
        $this->secret = $secret;
    }
}

/** Serializable 接口 → C: 自定义序列化（php-serialize 同样按 __PHP_Incomplete_Class 占位） */
class DemoToken implements Serializable
{
    public function __construct(
        private string $user,
        private int $expire,
    ) {}

    public function serialize(): ?string
    {
        return $this->user . '|' . $this->expire;
    }

    public function unserialize(string $data): void
    {
        [$this->user, $this->expire] = explode('|', $data);
    }

    // 仅为消除 PHP 8.1+ 对 Serializable 的弃用告警；serialize() 仍走 C: 格式
    public function __serialize(): array
    {
        return ['user' => $this->user, 'expire' => $this->expire];
    }

    public function __unserialize(array $data): void
    {
        $this->user = $data['user'];
        $this->expire = $data['expire'];
    }
}

// ---------- 样例构造 ----------

function user_sample(): DemoUser
{
    return new DemoUser(1001, 'Alice', true, 'alice@example.com', 'top-secret');
}

/** 覆盖标量 / 特殊浮点 / 多字节与二进制串 / 数组 / 对象 */
function build_samples(): array
{
    $m = [];
    $m[PREFIX . 'string'] = 'hello-phpserial';
    $m[PREFIX . 'string-utf8'] = '中文值-PhpSerial';
    $m[PREFIX . 'string-binary'] = "abc\x00\xff";   // s: 按字节计数，含 NUL / 高位字节
    $m[PREFIX . 'int'] = 42;
    $m[PREFIX . 'int-neg'] = -9876543210;
    $m[PREFIX . 'float'] = 3.14159;
    $m[PREFIX . 'float-inf'] = INF;
    $m[PREFIX . 'float-nan'] = NAN;
    $m[PREFIX . 'bool-true'] = true;
    $m[PREFIX . 'bool-false'] = false;
    $m[PREFIX . 'null'] = null;

    $m[PREFIX . 'list'] = ['a', 'b', '中文', 1];                       // 连续 0 起始 → 列表
    $m[PREFIX . 'map'] = ['k1' => 'v1', 'k2' => 7, '键' => '值'];       // 字符串键（含中文键）
    $m[PREFIX . 'mixed-keys'] = [0 => 'zero', 'k' => 'v', 2 => 'two']; // 数字/字符串混合键
    $m[PREFIX . 'nested'] = [
        'list' => [1, 2, ['n' => '中文']],
        'point' => new DemoPoint(9, 0),
        'raw' => "bin\x00ary",
    ];

    $m[PREFIX . 'point'] = new DemoPoint(3, 4);
    $m[PREFIX . 'person'] = new DemoPerson('Bob', 20, new DemoPoint(1, 2));
    $m[PREFIX . 'user'] = user_sample();
    $m[PREFIX . 'custom'] = new DemoToken('alice', 1735689600);
    return $m;
}

// ---------- 最小 RESP 客户端 ----------

/** 最小 RESP 客户端：AUTH / SET / DEL / HSET / RPUSH / SADD / ZADD 二进制值 */
class RedisCli
{
    /** @var resource */
    private $sock;

    public function __construct(string $host, int $port)
    {
        $sock = @stream_socket_client("tcp://$host:$port", $errno, $errstr, 10);
        if ($sock === false) {
            fwrite(STDERR, "连接 Redis 失败: $errstr ($errno)\n");
            exit(1);
        }
        stream_set_timeout($sock, 10);
        $this->sock = $sock;
    }

    public function close(): void
    {
        fclose($this->sock);
    }

    public function auth(string $password): void
    {
        $this->cmd(['AUTH', $password]);
        $this->readOk();
    }

    public function set(string $key, string $value): void
    {
        $this->cmd(['SET', $key, $value]);
        $this->readOk();
    }

    public function delete(string $key): void
    {
        $this->cmd(['DEL', $key]);
        $this->readInteger();
    }

    public function hset(string $key, string $field, string $value): void
    {
        $this->cmd(['HSET', $key, $field, $value]);
        $this->readInteger();
    }

    public function rpush(string $key, string $value): void
    {
        $this->cmd(['RPUSH', $key, $value]);
        $this->readInteger();
    }

    public function sadd(string $key, string $member): void
    {
        $this->cmd(['SADD', $key, $member]);
        $this->readInteger();
    }

    public function zadd(string $key, float $score, string $member): void
    {
        $this->cmd(['ZADD', $key, (string) $score, $member]);
        $this->readInteger();
    }

    private function cmd(array $parts): void
    {
        $buf = '*' . count($parts) . "\r\n";
        foreach ($parts as $p) {
            $buf .= '$' . strlen($p) . "\r\n" . $p . "\r\n";   // strlen 按字节，二进制安全
        }
        fwrite($this->sock, $buf);
    }

    private function readOk(): void
    {
        $line = $this->readLine();
        if ($line[0] === '+') return;
        if ($line[0] === '-') throw new RuntimeException('Redis error: ' . substr($line, 1));
        throw new RuntimeException("unexpected Redis reply: $line");
    }

    private function readInteger(): void
    {
        $line = $this->readLine();
        if ($line[0] === ':') return;
        if ($line[0] === '-') throw new RuntimeException('Redis error: ' . substr($line, 1));
        throw new RuntimeException("unexpected Redis reply: $line");
    }

    private function readLine(): string
    {
        $line = fgets($this->sock);
        if ($line === false) throw new RuntimeException('Redis connection closed');
        return rtrim($line, "\r\n");
    }
}

// ---------- 复合类型（字段级验证） ----------

/** Hash/List/Set/ZSet：字段值为 PHP serialize（混 UTF-8，测字段级 Auto） */
function seed_compound_types(RedisCli $redis): void
{
    $hashKey = PREFIX . 'hash';
    $redis->delete($hashKey);
    $redis->hset($hashKey, 'user', serialize(user_sample()));
    $redis->hset($hashKey, 'list', serialize(['a', 'b', '中文']));
    $redis->hset($hashKey, 'point', serialize(new DemoPoint(3, 4)));
    $redis->hset($hashKey, 'plain-utf8', '新增字段');
    echo "HSET $hashKey (user/list/point=PhpSerial, plain-utf8=UTF8)\n";

    $listKey = PREFIX . 'list-key';
    $redis->delete($listKey);
    $redis->rpush($listKey, serialize('hello-list'));
    $redis->rpush($listKey, serialize(42));
    $redis->rpush($listKey, serialize(user_sample()));
    $redis->rpush($listKey, '纯文本元素');
    echo "RPUSH $listKey (3×PhpSerial + 1×UTF8)\n";

    $setKey = PREFIX . 'set-key';
    $redis->delete($setKey);
    $redis->sadd($setKey, serialize('member-a'));
    $redis->sadd($setKey, serialize(['k' => '中文']));
    $redis->sadd($setKey, 'utf8-member');
    echo "SADD $setKey (2×PhpSerial + 1×UTF8)\n";

    $zsetKey = PREFIX . 'zset';
    $redis->delete($zsetKey);
    $redis->zadd($zsetKey, 1.0, serialize('z-low'));
    $redis->zadd($zsetKey, 2.5, serialize(user_sample()));
    $redis->zadd($zsetKey, 9.0, 'z-utf8');
    echo "ZADD $zsetKey (2×PhpSerial + 1×UTF8)\n";
}

// ---------- 入口 ----------

/** 命令行 > REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD > 本机默认 */
function redis_conn_from_argv(array $argv): array
{
    $host = $argv[1] ?? getenv('REDIS_SERVER') ?: '127.0.0.1';
    $port = (int) ($argv[2] ?? getenv('REDIS_PROT') ?: '6379');
    $password = $argv[3] ?? getenv('REDIS_PASSWORD') ?: 'hepengju';
    return [$host, $port, $password];
}

[$host, $port, $password] = redis_conn_from_argv($argv);
$redis = new RedisCli($host, $port);
if ($password !== '') {
    $redis->auth($password);
}
try {
    foreach (build_samples() as $key => $value) {
        $payload = serialize($value);
        $redis->set($key, $payload);
        echo "SET $key (" . strlen($payload) . " bytes)\n";
    }
    seed_compound_types($redis);
} finally {
    $redis->close();
}
echo "done. STRING → PhpSerial/Auto；Hash/List/Set/ZSet → 打开字段查看。\n";

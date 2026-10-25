package example.channels;

import io.lettuce.core.Consumer;
import io.lettuce.core.XReadArgs;
import io.lettuce.core.api.sync.RedisCommands;
import java.util.Map;
import redis.clients.jedis.JedisPooled;
import redis.clients.jedis.StreamEntryID;

// Redis streams with Jedis and Lettuce.
public class Streams {
    private final JedisPooled jedis;
    private final RedisCommands<String, String> lettuce;

    public Streams(JedisPooled jedis, RedisCommands<String, String> lettuce) {
        this.jedis = jedis;
        this.lettuce = lettuce;
    }

    public void record(String id) {
        jedis.xadd("events", StreamEntryID.NEW_ENTRY, Map.of("id", id));
        lettuce.xadd("audits", Map.of("id", id));
    }

    public void consume() {
        jedis.xreadGroup("g", "c", null, Map.of("events", StreamEntryID.UNRECEIVED_ENTRY));
        lettuce.xreadgroup(Consumer.from("g", "c"), XReadArgs.StreamOffset.lastConsumed("audits"));
    }
}

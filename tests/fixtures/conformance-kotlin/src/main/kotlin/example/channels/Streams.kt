package example.channels

import io.lettuce.core.Consumer
import io.lettuce.core.XReadArgs
import io.lettuce.core.api.sync.RedisCommands
import redis.clients.jedis.JedisPooled
import redis.clients.jedis.StreamEntryID

// Redis streams with Jedis and Lettuce.
class Streams(
    private val jedis: JedisPooled,
    private val lettuce: RedisCommands<String, String>,
) {
    fun record(id: String) {
        jedis.xadd("events", StreamEntryID.NEW_ENTRY, mapOf("id" to id))
        lettuce.xadd("audits", mapOf("id" to id))
    }

    fun consume() {
        jedis.xreadGroup("g", "c", null, mapOf("events" to StreamEntryID.UNRECEIVED_ENTRY))
        lettuce.xreadgroup(Consumer.from("g", "c"), XReadArgs.StreamOffset.lastConsumed("audits"))
    }
}

package com.fdxmp.app.media

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/** 一条编辑记录（设备 Tab / 编辑 Tab 共用）。 */
data class EditRecord(
    val uri: String,
    val name: String,
    val paramsJson: String,
    val presetName: String,
    val updatedAt: Long,
) {
    companion object {
        /** 默认参数（空 JSON，核心按默认值回落）。 */
        fun fresh(uri: String, name: String): EditRecord = EditRecord(
            uri = uri,
            name = name,
            paramsJson = "{\"version\":1}",
            presetName = "自定义",
            updatedAt = System.currentTimeMillis(),
        )
    }
}

/** 编辑记录的本地存储（SharedPreferences + JSON，量级小，够用）。 */
object EditStore {
    private const val PREFS_NAME = "fdxmp_edits"
    private const val KEY_RECORDS = "records"

    fun load(context: Context): List<EditRecord> {
        val raw = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .getString(KEY_RECORDS, null) ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            buildList {
                for (index in 0 until array.length()) {
                    val item = array.optJSONObject(index) ?: continue
                    add(
                        EditRecord(
                            uri = item.optString("uri"),
                            name = item.optString("name"),
                            paramsJson = item.optString("params", "{\"version\":1}"),
                            presetName = item.optString("preset", "自定义"),
                            updatedAt = item.optLong("updatedAt", 0L),
                        ),
                    )
                }
            }.sortedByDescending { it.updatedAt }
        }.getOrDefault(emptyList())
    }

    fun find(context: Context, uri: String): EditRecord? =
        load(context).firstOrNull { it.uri == uri }

    fun upsert(context: Context, record: EditRecord) {
        val records = load(context).filterNot { it.uri == record.uri } + record
        save(context, records)
    }

    fun remove(context: Context, uri: String) {
        save(context, load(context).filterNot { it.uri == uri })
    }

    private fun save(context: Context, records: List<EditRecord>) {
        val array = JSONArray()
        records.forEach { record ->
            array.put(
                JSONObject().apply {
                    put("uri", record.uri)
                    put("name", record.name)
                    put("params", record.paramsJson)
                    put("preset", record.presetName)
                    put("updatedAt", record.updatedAt)
                },
            )
        }
        context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .edit()
            .putString(KEY_RECORDS, array.toString())
            .apply()
    }
}

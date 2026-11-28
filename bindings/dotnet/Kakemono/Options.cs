using System.Collections.Generic;
using System.IO;
using System.Text.Json;

namespace Kakemono
{
    /// <summary>
    /// Options given to kakemono: each set option is written as JSON for the native library, under
    /// the name the other bindings use (camelCase). An option left null takes kakemono's default.
    /// </summary>
    public abstract class Options
    {
        private protected Options()
        {
        }

        /// <summary>Adds the set options to <paramref name="values"/>.</summary>
        internal abstract void Fill(Dictionary<string, object> values);

        /// <summary>
        /// The options as JSON text for the C ABI (UTF-8, then a NUL), or null. Written by hand,
        /// without reflection, so that it works when trimmed or compiled ahead of time.
        /// </summary>
        internal static byte[]? ToJson(Options? options)
        {
            if (options is null)
            {
                return null;
            }
            var values = new Dictionary<string, object>();
            options.Fill(values);
            using var buffer = new MemoryStream();
            using (var json = new Utf8JsonWriter(buffer))
            {
                json.WriteStartObject();
                foreach (KeyValuePair<string, object> entry in values)
                {
                    switch (entry.Value)
                    {
                        case string text:
                            json.WriteString(entry.Key, text);
                            break;
                        case bool flag:
                            json.WriteBoolean(entry.Key, flag);
                            break;
                        case int count:
                            json.WriteNumber(entry.Key, count);
                            break;
                        case double number:
                            if (double.IsNaN(number) || double.IsInfinity(number))
                            {
                                throw new KakemonoException($"{entry.Key} must be a finite number", KakemonoErrorKind.InvalidArgument);
                            }
                            json.WriteNumber(entry.Key, number);
                            break;
                    }
                }
                json.WriteEndObject();
            }
            buffer.WriteByte(0);
            return buffer.ToArray();
        }
    }
}

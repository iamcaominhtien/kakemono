using System.Collections.Generic;
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

        /// <summary>The options as JSON text for the C ABI (UTF-8, then a NUL), or null.</summary>
        internal static byte[]? ToJson(Options? options)
        {
            if (options is null)
            {
                return null;
            }
            var values = new Dictionary<string, object>();
            options.Fill(values);
            return Native.Utf8(JsonSerializer.Serialize(values));
        }
    }
}

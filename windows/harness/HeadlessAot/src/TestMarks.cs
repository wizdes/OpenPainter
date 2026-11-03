// The marks the test source uses, with the same names. This build has no xunit runner, and
// Runner.cs reads these marks to find each test.
namespace Xunit
{
    [AttributeUsage(AttributeTargets.Method)]
    public class FactAttribute : Attribute;

    [AttributeUsage(AttributeTargets.Method)]
    public sealed class TheoryAttribute : Attribute;

    [AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
    public sealed class InlineDataAttribute(params object?[]? values) : Attribute
    {
        public object?[] Values { get; } = values ?? [null];
    }
}

namespace Avalonia.Headless.XUnit
{
    [AttributeUsage(AttributeTargets.Method)]
    public sealed class AvaloniaFactAttribute : Attribute;

    [AttributeUsage(AttributeTargets.Method)]
    public sealed class AvaloniaTheoryAttribute : Attribute;
}

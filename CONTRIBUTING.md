# Contributing to KFtray

👋 Thanks for your interest in contributing to KFtray! We're glad you want to help out and we look forward to your involvement in our community. Here are some guidelines to help you get started.

## How to Contribute

### Reporting Bugs or Issues

- Check the [Issues](https://github.com/hcavarsan/kftray/issues) page to make sure it hasn't already been reported.
- Click on the 'New Issue' button and choose the appropriate template.
- Fill out all necessary fields with as much information as possible.
- The more information you can provide, the more likely we'll be able to help.

### Suggesting Enhancements

- Look through the [Issues](https://github.com/hcavarsan/kftray/issues) to ensure your suggestion doesn't already exist.
- Create a new issue, select the feature request template, and detail your idea.

### Pull Requests

- Fork the repository and create your branch from `main`.
- If you've added code that should be tested, add some tests.
- Ensure your code is formatted and linted (pre-commit hook handles this automatically).
- Write clear, descriptive commit messages.
- Open a new pull request with a clear title and description.

### Setting Up Your Development Environment

KFtray uses [mise](https://mise.jdx.dev) to manage the development environment. This ensures all contributors use the same tool versions.

1. **Install mise:**

   ```bash
   curl https://mise.run | sh
   ```

2. **Clone and setup:**

   ```bash
   git clone https://github.com/hcavarsan/kftray.git
   cd kftray
   mise install        # Install all required tools
   mise run setup      # Setup system dependencies
   ```

3. **Start developing:**

   ```bash
   mise run dev        # Launch development mode
   ```

The project has an automatic Git pre-commit hook that runs formatting and linting on every commit. Your code will be automatically formatted and checked before committing.

### Available Development Commands

- `mise run dev` - Start development mode
- `mise run build` - Build production app
- `mise run format` - Format all code
- `mise run lint` - Lint with auto-fix
- `mise run test:back` - Run backend tests
- `mise run precommit` - Run all checks (format, lint, test)

Run `mise tasks` to see all available commands.

For detailed build instructions, see [Building from Source](docs/kftray/BUILD.md).

## Code of Conduct

Participation in the KFtray community is governed by a [Code of Conduct](CODE_OF_CONDUCT.md). Please read it before contributing to make sure you understand our community standards.

## Asking Questions

We welcome any developer to ask questions and get help from our community. Just make an issue with the tag 'question' and we'll get back to you as soon as we can.

Once again, thanks for your interest in KFtray. We're excited to see what you bring to our community!

KFtray is distributed under the [MIT License](LICENSE.md).

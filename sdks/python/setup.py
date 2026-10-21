#!/usr/bin/env python3
"""
Setup script for Wrfcoin Python SDK

This setup.py is maintained for compatibility with older build systems.
The main configuration is in pyproject.toml.
"""

from setuptools import setup, find_packages
import os

# Read version from package
def get_version():
    here = os.path.abspath(os.path.dirname(__file__))
    version_file = os.path.join(here, "wrfcoin", "__init__.py")
    
    with open(version_file, "r", encoding="utf-8") as f:
        for line in f:
            if line.startswith("__version__"):
                return line.split("=")[1].strip().strip('"').strip("'")
    
    raise RuntimeError("Unable to find version string")

# Read README for long description
def get_long_description():
    here = os.path.abspath(os.path.dirname(__file__))
    readme_file = os.path.join(here, "README.md")
    
    try:
        with open(readme_file, "r", encoding="utf-8") as f:
            return f.read()
    except FileNotFoundError:
        return "Python SDK for Wrfcoin blockchain ecosystem"

setup(
    name="wrfcoin",
    version=get_version(),
    description="Python SDK for Wrfcoin blockchain ecosystem",
    long_description=get_long_description(),
    long_description_content_type="text/markdown",
    author="Wrfcoin Team",
    author_email="dev@wrfcoin.io",
    url="https://github.com/wrfcoin/wrfcoin-sdk-python",
    project_urls={
        "Documentation": "https://docs.wrfcoin.io/sdk/python",
        "Source": "https://github.com/wrfcoin/wrfcoin-sdk-python",
        "Tracker": "https://github.com/wrfcoin/wrfcoin-sdk-python/issues",
    },
    packages=find_packages(),
    python_requires=">=3.8",
    install_requires=[
        "aiohttp>=3.9.0",
        "websockets>=12.0",
        "gql[aiohttp,websockets]>=3.5.0",
    ],
    extras_require={
        "validation": ["pydantic>=2.0.0"],
        "cli": ["click>=8.0.0"],
        "dev": [
            "pytest>=7.0.0",
            "pytest-asyncio>=0.21.0",
            "pytest-cov>=4.0.0",
            "black>=23.0.0",
            "isort>=5.12.0",
            "flake8>=6.0.0",
            "mypy>=1.0.0",
        ],
        "docs": [
            "sphinx>=6.0.0",
            "sphinx-rtd-theme>=1.3.0",
        ],
    },
    entry_points={
        "console_scripts": [
            "wrfcoin=wrfcoin.cli:main",
        ],
    },
    classifiers=[
        "Development Status :: 4 - Beta",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "Topic :: Internet :: WWW/HTTP",
        "Topic :: Office/Business :: Financial",
    ],
    keywords="wrfcoin blockchain weather nft carbon credits governance sdk",
    license="MIT",
    zip_safe=False,
)
